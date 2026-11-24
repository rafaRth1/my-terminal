//! Claude accounts for new panes.
//!
//! Every account shares the same `~/.claude` configuration; only the login
//! changes. The `Default` account uses the login stored by `claude` itself.
//! Each extra account is a file `<name>.token` in [`default_dir`] holding a
//! long-lived token from `claude setup-token`; panes spawned while it is
//! active get it as [`TOKEN_ENV`]. Running panes keep the account they
//! started with.

use std::fmt;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

/// Environment variable Claude Code reads its OAuth token from.
pub const TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";
/// Name of the account that uses the login stored by `claude`.
pub const DEFAULT_ACCOUNT: &str = "Default";
const TOKEN_EXTENSION: &str = "token";
/// Longest token file accepted; real tokens are a few hundred bytes.
const MAX_TOKEN_BYTES: u64 = 4096;
/// Permission bits a token file may have: owner only.
const OWNER_ONLY: u32 = 0o700;
/// Mode of token files the app writes.
const TOKEN_FILE_MODE: u32 = 0o600;
/// Longest account name; it is also the file name.
const MAX_NAME_CHARS: usize = 32;

/// `$XDG_CONFIG_HOME/my-terminal/accounts`, or `~/.config/my-terminal/accounts`.
pub fn default_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("my-terminal").join("accounts"))
}

/// What a new pane starts with. `Debug` shows the variable name, never the
/// token.
#[derive(Default, PartialEq, Eq)]
pub struct PaneAccount {
    /// Environment entry to add; `None` keeps the stored login.
    pub env: Option<(String, String)>,
    /// Account name shown on the pane; `None` for `Default`.
    pub badge: Option<String>,
}

impl fmt::Debug for PaneAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaneAccount")
            .field(
                "env",
                &self.env.as_ref().map(|(key, _)| (key, "<redacted>")),
            )
            .field("badge", &self.badge)
            .finish()
    }
}

/// The accounts found on disk and the one new panes use.
#[derive(Debug)]
pub struct Accounts {
    dir: Option<PathBuf>,
    /// Token accounts sorted by name; `Default` is implicit and comes first.
    tokens: Vec<TokenAccount>,
    /// 0 is `Default`, `n` is `tokens[n - 1]`.
    active: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenAccount {
    name: String,
    path: PathBuf,
}

/// Why a token file was skipped or could not be used.
#[derive(Debug, PartialEq, Eq)]
pub enum AccountError {
    /// Group or others can read or write the file.
    OpenPermissions {
        path: PathBuf,
        mode: u32,
    },
    /// The file holds no token.
    Empty {
        path: PathBuf,
    },
    /// Not one line of printable text, or too long.
    Malformed {
        path: PathBuf,
    },
    /// A directory, FIFO or other non-regular file.
    NotAFile {
        path: PathBuf,
    },
    Io {
        path: PathBuf,
        kind: io::ErrorKind,
    },
    /// The account name cannot be used as a file name or is reserved.
    InvalidName {
        reason: &'static str,
    },
    /// The pasted text is not one token from `claude setup-token`.
    InvalidToken {
        reason: &'static str,
    },
    /// Another account already has this name.
    Exists {
        name: String,
    },
    /// No token account has this name.
    UnknownAccount {
        name: String,
    },
    /// Neither `XDG_CONFIG_HOME` nor `HOME` is set.
    NoDirectory,
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenPermissions { path, mode } => write!(
                f,
                "{} has mode {mode:03o}; run `chmod 600` on it",
                path.display()
            ),
            Self::Empty { path } => write!(f, "{} is empty", path.display()),
            Self::Malformed { path } => write!(
                f,
                "{} must hold only the token from `claude setup-token`",
                path.display()
            ),
            Self::NotAFile { path } => write!(f, "{} is not a regular file", path.display()),
            Self::Io { path, kind } => write!(f, "cannot use {}: {kind}", path.display()),
            Self::InvalidName { reason } | Self::InvalidToken { reason } => f.write_str(reason),
            Self::Exists { name } => write!(
                f,
                "{name}.{TOKEN_EXTENSION} already exists in the accounts folder; choose another name"
            ),
            Self::UnknownAccount { name } => write!(f, "no account named {name}"),
            Self::NoDirectory => f.write_str("cannot find the config directory (HOME is not set)"),
        }
    }
}

impl std::error::Error for AccountError {}

impl Accounts {
    /// Scans `dir` for `*.token` files. A missing directory means only
    /// `Default`. Returns the files that were skipped and why.
    pub fn load(dir: Option<PathBuf>) -> (Self, Vec<AccountError>) {
        let mut accounts = Self {
            dir,
            tokens: Vec::new(),
            active: 0,
        };
        let skipped = accounts.rescan();
        (accounts, skipped)
    }

    /// Re-reads the directory, keeping the active account by name. Falls back
    /// to `Default` when the active token file is gone.
    pub fn rescan(&mut self) -> Vec<AccountError> {
        let active = self.active_name().to_owned();
        let (tokens, skipped) = self.dir.as_deref().map_or_else(Default::default, scan);
        self.tokens = tokens;
        self.active = self
            .tokens
            .iter()
            .position(|account| account.name == active)
            .map_or(0, |index| index + 1);
        skipped
    }

    /// Makes the next account active, wrapping back to `Default`.
    pub fn switch_next(&mut self) {
        self.active = (self.active + 1) % (self.tokens.len() + 1);
    }

    pub fn active_name(&self) -> &str {
        match self.active {
            0 => DEFAULT_ACCOUNT,
            n => &self.tokens[n - 1].name,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.tokens.len() + 1
    }

    /// Every account name, `Default` first.
    pub fn names(&self) -> Vec<&str> {
        std::iter::once(DEFAULT_ACCOUNT)
            .chain(self.tokens.iter().map(|account| account.name.as_str()))
            .collect()
    }

    /// Makes `name` the account for new panes; `false` if there is none.
    pub fn select(&mut self, name: &str) -> bool {
        if name == DEFAULT_ACCOUNT {
            self.active = 0;
            return true;
        }
        match self.position(name) {
            Some(index) => {
                self.active = index + 1;
                true
            }
            None => false,
        }
    }

    /// Saves a new account as an owner-only token file. The active account
    /// does not change.
    pub fn add(&mut self, name: &str, token: &str) -> Result<(), AccountError> {
        let name = valid_name(name)?;
        let token = clean_token(token).map_err(|reason| AccountError::InvalidToken { reason })?;
        let dir = self.dir.clone().ok_or(AccountError::NoDirectory)?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(&dir)
            .map_err(|err| io_error(&dir, &err))?;
        make_private(&dir)?;
        write_new(&dir, name, token)?;
        self.rescan();
        Ok(())
    }

    /// Deletes a token account. New panes fall back to `Default` if it was
    /// active; running panes keep the token they started with.
    pub fn remove(&mut self, name: &str) -> Result<(), AccountError> {
        let path = self.path_of(name)?;
        fs::remove_file(&path).map_err(|err| io_error(&path, &err))?;
        self.rescan();
        Ok(())
    }

    /// Renames a token account, keeping it active if it was.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), AccountError> {
        let from = self.path_of(old)?;
        let new = valid_name(new)?;
        let to = from.with_file_name(format!("{new}.{TOKEN_EXTENSION}"));
        if from != to {
            // A hard link never replaces an existing file, unlike `rename`.
            fs::hard_link(&from, &to).map_err(|err| exists_or_io(new, &to, &err))?;
            if let Err(err) = fs::remove_file(&from) {
                // Both names exist now; show that instead of a stale list.
                self.rescan();
                return Err(io_error(&from, &err));
            }
        }
        let was_active = self.active_name() == old;
        self.rescan();
        if was_active {
            self.select(new);
        }
        Ok(())
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.tokens.iter().position(|account| account.name == name)
    }

    fn path_of(&self, name: &str) -> Result<PathBuf, AccountError> {
        self.position(name)
            .map(|index| self.tokens[index].path.clone())
            .ok_or_else(|| AccountError::UnknownAccount {
                name: name.to_owned(),
            })
    }

    /// Account for a new pane. The badge is set exactly when the token is.
    pub fn pane_account(&self) -> Result<PaneAccount, AccountError> {
        Ok(match self.active_env()? {
            Some(env) => PaneAccount {
                env: Some(env),
                badge: Some(self.active_name().to_owned()),
            },
            None => PaneAccount::default(),
        })
    }

    /// Account `name` for a resumed pane, without changing the active one.
    pub fn pane_account_named(&self, name: &str) -> Result<PaneAccount, AccountError> {
        if name == DEFAULT_ACCOUNT {
            return Ok(PaneAccount::default());
        }
        let token = read_token(&self.path_of(name)?)?;
        Ok(PaneAccount {
            env: Some((TOKEN_ENV.to_owned(), token)),
            badge: Some(name.to_owned()),
        })
    }

    /// Environment entry for a new pane: `None` for `Default`, otherwise
    /// [`TOKEN_ENV`] with the token read from disk now.
    pub fn active_env(&self) -> Result<Option<(String, String)>, AccountError> {
        let Some(account) = self.active.checked_sub(1).map(|n| &self.tokens[n]) else {
            return Ok(None);
        };
        read_token(&account.path).map(|token| Some((TOKEN_ENV.to_owned(), token)))
    }
}

fn scan(dir: &Path) -> (Vec<TokenAccount>, Vec<AccountError>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return Default::default();
    };
    let mut tokens = Vec::new();
    let mut skipped = Vec::new();
    for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
        if path.extension().is_none_or(|ext| ext != TOKEN_EXTENSION) || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if name.is_empty() || name.eq_ignore_ascii_case(DEFAULT_ACCOUNT) {
            continue;
        }
        match check_permissions(&path) {
            Ok(()) => tokens.push(TokenAccount {
                name: name.to_owned(),
                path,
            }),
            Err(err) => skipped.push(err),
        }
    }
    tokens.sort_by(|a, b| a.name.cmp(&b.name));
    (tokens, skipped)
}

/// Trimmed name if it is safe as a file name and not reserved.
fn valid_name(name: &str) -> Result<&str, AccountError> {
    let name = name.trim();
    let invalid = |reason| Err(AccountError::InvalidName { reason });
    if name.is_empty() {
        return invalid("enter a name for the account");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return invalid("use at most 32 characters for the name");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return invalid("use only letters, digits, - and _ in the name");
    }
    if name.eq_ignore_ascii_case(DEFAULT_ACCOUNT) {
        return invalid("Default is the stored login; choose another name");
    }
    Ok(name)
}

/// The token without surrounding whitespace, or why it cannot be one.
fn clean_token(text: &str) -> Result<&str, &'static str> {
    let too_long = Err("the token is too long; paste only the token");
    if text.len() as u64 > MAX_TOKEN_BYTES {
        return too_long;
    }
    let token = text.trim();
    // Saved files end with a line break and must fit in MAX_TOKEN_BYTES.
    if token.len() as u64 >= MAX_TOKEN_BYTES {
        return too_long;
    }
    if token.is_empty() {
        return Err("paste the token from `claude setup-token`");
    }
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("the token must be one word; paste only the token");
    }
    Ok(token)
}

/// Writes `<name>.token` with mode 0600 without ever replacing a file: the
/// token goes to a hidden temp file first, which is then hard-linked into
/// place.
fn write_new(dir: &Path, name: &str, token: &str) -> Result<(), AccountError> {
    let path = dir.join(format!("{name}.{TOKEN_EXTENSION}"));
    let temp = dir.join(format!(
        ".{name}.{TOKEN_EXTENSION}.{}.tmp",
        std::process::id()
    ));
    // `create_new` fails on any existing entry, symlinks included, so the
    // temp file below is always ours and safe to remove.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(TOKEN_FILE_MODE)
        .open(&temp)
        .map_err(|err| io_error(&temp, &err))?;
    let written = file
        .write_all(token.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|err| io_error(&temp, &err))
        .and_then(|()| fs::hard_link(&temp, &path).map_err(|err| exists_or_io(name, &path, &err)));
    drop(file);
    let _ = fs::remove_file(&temp);
    written
}

/// Makes `dir` owner-only. Only its owner (or root) may change its mode, so
/// a directory owned by someone else fails here instead of being trusted.
fn make_private(dir: &Path) -> Result<(), AccountError> {
    let metadata = fs::metadata(dir).map_err(|err| io_error(dir, &err))?;
    if metadata.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(OWNER_ONLY))
            .map_err(|err| io_error(dir, &err))?;
    }
    Ok(())
}

fn exists_or_io(name: &str, path: &Path, err: &io::Error) -> AccountError {
    if err.kind() == io::ErrorKind::AlreadyExists {
        AccountError::Exists {
            name: name.to_owned(),
        }
    } else {
        io_error(path, err)
    }
}

fn io_error(path: &Path, err: &io::Error) -> AccountError {
    AccountError::Io {
        path: path.to_owned(),
        kind: err.kind(),
    }
}

/// Owner-only check. Follows symlinks, so a link is judged by its target.
fn check_permissions(path: &Path) -> Result<(), AccountError> {
    let metadata = fs::metadata(path).map_err(|err| io_error(path, &err))?;
    check_mode(path, &metadata)
}

fn check_mode(path: &Path, metadata: &fs::Metadata) -> Result<(), AccountError> {
    let mode = metadata.permissions().mode() & 0o777;
    if mode & !OWNER_ONLY == 0 {
        Ok(())
    } else {
        Err(AccountError::OpenPermissions {
            path: path.to_owned(),
            mode,
        })
    }
}

/// Checks the type and mode and reads through the same open handle, so the
/// file cannot be swapped between the check and the read.
fn read_token(path: &Path) -> Result<String, AccountError> {
    // Checked before opening too: opening a FIFO would block the UI thread.
    let not_a_file = || AccountError::NotAFile {
        path: path.to_owned(),
    };
    if !fs::metadata(path)
        .map_err(|err| io_error(path, &err))?
        .is_file()
    {
        return Err(not_a_file());
    }
    let file = fs::File::open(path).map_err(|err| io_error(path, &err))?;
    let metadata = file.metadata().map_err(|err| io_error(path, &err))?;
    if !metadata.is_file() {
        return Err(not_a_file());
    }
    check_mode(path, &metadata)?;

    let mut text = String::new();
    file.take(MAX_TOKEN_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| io_error(path, &err))?;
    if text.trim().is_empty() {
        return Err(AccountError::Empty {
            path: path.to_owned(),
        });
    }
    clean_token(&text)
        .map(str::to_owned)
        .map_err(|_| AccountError::Malformed {
            path: path.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "my-terminal-accounts-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn token(&self, file: &str, contents: &str, mode: u32) -> PathBuf {
            let path = self.0.join(file);
            fs::write(&path, contents).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(accounts: &mut Accounts) -> Vec<String> {
        (0..accounts.len())
            .map(|_| {
                let name = accounts.active_name().to_owned();
                accounts.switch_next();
                name
            })
            .collect()
    }

    #[test]
    fn missing_directory_has_only_default() {
        let (accounts, skipped) = Accounts::load(Some(PathBuf::from("/nonexistent/my-terminal")));
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT);
        assert_eq!(accounts.len(), 1);
        assert_eq!(skipped, []);
        assert_eq!(accounts.active_env(), Ok(None));

        let (accounts, _) = Accounts::load(None);
        assert_eq!(accounts.len(), 1);
    }

    #[test]
    fn token_files_become_sorted_accounts() {
        let dir = TempDir::new();
        dir.token("B.token", "tok-b\n", 0o600);
        dir.token("A2.token", "tok-a2", 0o400);
        dir.token("notes.txt", "x", 0o600);
        dir.token("default.token", "x", 0o600);
        fs::create_dir(dir.0.join("dir.token")).unwrap();

        let (mut accounts, skipped) = Accounts::load(Some(dir.0.clone()));
        assert_eq!(skipped, []);
        assert_eq!(names(&mut accounts), ["Default", "A2", "B"]);
    }

    #[test]
    fn open_permissions_are_skipped_with_reason() {
        let dir = TempDir::new();
        let path = dir.token("B.token", "tok", 0o644);

        let (accounts, skipped) = Accounts::load(Some(dir.0.clone()));
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            skipped,
            [AccountError::OpenPermissions {
                path: path.clone(),
                mode: 0o644
            }]
        );
        assert!(skipped[0].to_string().contains("chmod 600"));
    }

    #[test]
    fn symlink_is_judged_by_its_target() {
        let dir = TempDir::new();
        let target = dir.token("shared.txt", "tok", 0o644);
        std::os::unix::fs::symlink(&target, dir.0.join("B.token")).unwrap();

        let (accounts, skipped) = Accounts::load(Some(dir.0.clone()));
        assert_eq!(accounts.len(), 1);
        assert!(matches!(
            skipped.as_slice(),
            [AccountError::OpenPermissions { mode: 0o644, .. }]
        ));
    }

    #[test]
    fn symlink_to_private_file_is_accepted() {
        let dir = TempDir::new();
        let target = dir.token("shared.txt", "tok", 0o600);
        std::os::unix::fs::symlink(&target, dir.0.join("B.token")).unwrap();

        let (mut accounts, skipped) = Accounts::load(Some(dir.0.clone()));
        assert_eq!(skipped, []);
        accounts.switch_next();
        assert_eq!(accounts.active_name(), "B");
        assert_eq!(
            accounts.active_env(),
            Ok(Some((TOKEN_ENV.to_owned(), "tok".to_owned())))
        );
    }

    #[test]
    fn only_owner_bits_are_allowed() {
        let dir = TempDir::new();
        dir.token("Owner.token", "tok", 0o700);
        dir.token("Other.token", "tok", 0o604);
        dir.token("Group.token", "tok", 0o620);

        let (mut accounts, skipped) = Accounts::load(Some(dir.0.clone()));
        assert_eq!(names(&mut accounts), ["Default", "Owner"]);
        let mut modes: Vec<u32> = skipped
            .iter()
            .map(|err| match err {
                AccountError::OpenPermissions { mode, .. } => *mode,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        modes.sort_unstable();
        assert_eq!(modes, [0o604, 0o620]);
    }

    #[test]
    fn token_must_be_one_printable_word() {
        let dir = TempDir::new();
        let path = dir.token("B.token", "tok\nextra", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.switch_next();
        assert_eq!(
            accounts.active_env(),
            Err(AccountError::Malformed { path: path.clone() })
        );

        fs::write(&path, "x".repeat(5000)).unwrap();
        assert_eq!(
            accounts.active_env(),
            Err(AccountError::Malformed { path: path.clone() }),
            "too long"
        );

        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(accounts.active_env(), Err(AccountError::NotAFile { path }));
    }

    #[test]
    fn errors_never_contain_the_token() {
        let secret = "secret-token-value";
        let dir = TempDir::new();
        let path = dir.token("B.token", secret, 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.switch_next();

        let mut errors = Vec::new();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        errors.push(accounts.active_env().unwrap_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&path, format!("{secret} {secret}")).unwrap();
        errors.push(accounts.active_env().unwrap_err());
        fs::remove_file(&path).unwrap();
        errors.push(accounts.active_env().unwrap_err());

        for err in errors {
            assert!(!err.to_string().contains(secret), "{err}");
            assert!(!format!("{err:?}").contains(secret), "{err:?}");
        }
    }

    #[test]
    fn pane_badge_is_set_exactly_with_the_token() {
        let dir = TempDir::new();
        let path = dir.token("B.token", "tok-b", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));

        assert_eq!(accounts.pane_account(), Ok(PaneAccount::default()));
        accounts.switch_next();
        assert_eq!(
            accounts.pane_account(),
            Ok(PaneAccount {
                env: Some((TOKEN_ENV.to_owned(), "tok-b".to_owned())),
                badge: Some("B".to_owned()),
            })
        );

        // An unreadable token is an error, never a silent Default pane.
        fs::remove_file(&path).unwrap();
        assert!(matches!(
            accounts.pane_account(),
            Err(AccountError::Io { .. })
        ));
    }

    #[test]
    fn switch_wraps_and_env_follows_active() {
        let dir = TempDir::new();
        dir.token("B.token", "  tok-b \n", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));

        assert_eq!(accounts.active_env(), Ok(None));
        accounts.switch_next();
        assert_eq!(accounts.active_name(), "B");
        assert_eq!(
            accounts.active_env(),
            Ok(Some((TOKEN_ENV.to_owned(), "tok-b".to_owned())))
        );
        accounts.switch_next();
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT);
    }

    #[test]
    fn empty_or_loosened_token_is_an_error_at_spawn() {
        let dir = TempDir::new();
        let path = dir.token("B.token", " \n", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.switch_next();
        assert_eq!(
            accounts.active_env(),
            Err(AccountError::Empty { path: path.clone() })
        );

        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(
            accounts.active_env(),
            Err(AccountError::OpenPermissions { mode: 0o640, .. })
        ));
    }

    #[test]
    fn rescan_keeps_active_by_name_or_falls_back() {
        let dir = TempDir::new();
        dir.token("B.token", "tok-b", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.switch_next();

        dir.token("A.token", "tok-a", 0o600);
        accounts.rescan();
        assert_eq!(accounts.active_name(), "B", "new file sorts before B");
        assert_eq!(accounts.len(), 3);

        fs::remove_file(dir.0.join("B.token")).unwrap();
        accounts.rescan();
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT);
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn names_lists_default_first_without_changing_active() {
        let dir = TempDir::new();
        dir.token("B.token", "tok-b", 0o600);
        dir.token("A.token", "tok-a", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.switch_next();
        assert_eq!(accounts.names(), ["Default", "A", "B"]);
        assert_eq!(accounts.active_name(), "A");
    }

    #[test]
    fn select_picks_an_account_by_name() {
        let dir = TempDir::new();
        dir.token("B.token", "tok-b", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        assert!(accounts.select("B"));
        assert_eq!(accounts.active_name(), "B");
        assert!(!accounts.select("missing"));
        assert_eq!(accounts.active_name(), "B", "unknown name keeps active");
        assert!(accounts.select(DEFAULT_ACCOUNT));
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT);
    }

    #[test]
    fn add_writes_an_owner_only_token_file() {
        let dir = TempDir::new();
        let accounts_dir = dir.0.join("my-terminal").join("accounts");
        let (mut accounts, _) = Accounts::load(Some(accounts_dir.clone()));

        accounts.add("  Work_2 ", "  tok-work \n").unwrap();

        let path = accounts_dir.join("Work_2.token");
        assert_eq!(fs::read_to_string(&path).unwrap(), "tok-work\n");
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(mode_of(&accounts_dir), 0o700);
        assert_eq!(accounts.names(), ["Default", "Work_2"]);
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT, "add keeps active");
        let leftovers: Vec<_> = fs::read_dir(&accounts_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["Work_2.token"], "no temp file left");
    }

    #[test]
    fn add_refuses_bad_names_tokens_and_duplicates() {
        let dir = TempDir::new();
        dir.token("B.token", "tok-b", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));

        for name in [
            "",
            "  ",
            "a/b",
            "../x",
            ".hidden",
            "with space",
            "default",
            "DEFAULT",
        ] {
            assert!(
                matches!(
                    accounts.add(name, "tok"),
                    Err(AccountError::InvalidName { .. })
                ),
                "{name:?}"
            );
        }
        assert!(matches!(
            accounts.add(&"x".repeat(33), "tok"),
            Err(AccountError::InvalidName { .. })
        ));
        for token in ["", " \n", "two words", "tab\there", &"t".repeat(5000)] {
            assert!(
                matches!(
                    accounts.add("C", token),
                    Err(AccountError::InvalidToken { .. })
                ),
                "{token:?}"
            );
        }
        assert_eq!(
            accounts.add("B", "tok-new"),
            Err(AccountError::Exists { name: "B".into() })
        );
        assert_eq!(fs::read_to_string(dir.0.join("B.token")).unwrap(), "tok-b");
        assert_eq!(accounts.names(), ["Default", "B"]);
    }

    #[test]
    fn add_without_directory_fails() {
        let (mut accounts, _) = Accounts::load(None);
        assert_eq!(accounts.add("B", "tok"), Err(AccountError::NoDirectory));
    }

    #[test]
    fn remove_deletes_the_file_and_falls_back_to_default() {
        let dir = TempDir::new();
        let path = dir.token("B.token", "tok-b", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.select("B");

        accounts.remove("B").unwrap();
        assert!(!path.exists());
        assert_eq!(accounts.names(), ["Default"]);
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT);

        assert_eq!(
            accounts.remove("B"),
            Err(AccountError::UnknownAccount { name: "B".into() })
        );
        assert_eq!(
            accounts.remove(DEFAULT_ACCOUNT),
            Err(AccountError::UnknownAccount {
                name: DEFAULT_ACCOUNT.into()
            })
        );
    }

    #[test]
    fn rename_moves_the_file_and_keeps_active() {
        let dir = TempDir::new();
        dir.token("B.token", "tok-b", 0o600);
        dir.token("C.token", "tok-c", 0o600);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.select("B");

        accounts.rename("B", " Work ").unwrap();
        assert!(!dir.0.join("B.token").exists());
        let path = dir.0.join("Work.token");
        assert_eq!(fs::read_to_string(&path).unwrap(), "tok-b");
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(accounts.names(), ["Default", "C", "Work"]);
        assert_eq!(accounts.active_name(), "Work");

        assert_eq!(
            accounts.rename("Work", "C"),
            Err(AccountError::Exists { name: "C".into() })
        );
        assert_eq!(fs::read_to_string(dir.0.join("C.token")).unwrap(), "tok-c");
        assert!(matches!(
            accounts.rename("Work", "a/b"),
            Err(AccountError::InvalidName { .. })
        ));
        assert_eq!(
            accounts.rename("missing", "X"),
            Err(AccountError::UnknownAccount {
                name: "missing".into()
            })
        );
        assert_eq!(accounts.names(), ["Default", "C", "Work"]);
    }

    #[test]
    fn add_never_deletes_a_file_it_did_not_create() {
        let dir = TempDir::new();
        let planted = dir.0.join(format!(".B.token.{}.tmp", std::process::id()));
        let target = dir.token("other.txt", "x", 0o600);
        std::os::unix::fs::symlink(&target, &planted).unwrap();
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));

        assert!(matches!(
            accounts.add("B", "tok"),
            Err(AccountError::Io { .. })
        ));
        assert!(fs::symlink_metadata(&planted).is_ok(), "planted link kept");
        assert_eq!(fs::read_to_string(&target).unwrap(), "x");
    }

    #[test]
    fn add_tightens_a_loose_directory() {
        let dir = TempDir::new();
        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o777)).unwrap();
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        accounts.add("B", "tok").unwrap();
        assert_eq!(mode_of(&dir.0), 0o700);
    }

    #[test]
    fn longest_accepted_token_reads_back() {
        let dir = TempDir::new();
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        let longest = "t".repeat(usize::try_from(MAX_TOKEN_BYTES).unwrap() - 1);
        accounts.add("B", &longest).unwrap();
        accounts.select("B");
        assert_eq!(
            accounts.active_env(),
            Ok(Some((TOKEN_ENV.to_owned(), longest.clone())))
        );
        assert!(matches!(
            accounts.add("C", &format!("{longest}t")),
            Err(AccountError::InvalidToken { .. })
        ));
    }

    #[test]
    fn exists_error_names_the_file() {
        let dir = TempDir::new();
        dir.token("B.token", "tok", 0o644);
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        let err = accounts.add("B", "tok").unwrap_err();
        assert!(err.to_string().contains("B.token"), "{err}");
    }

    #[test]
    fn pane_account_debug_hides_the_token() {
        let account = PaneAccount {
            env: Some((TOKEN_ENV.to_owned(), "secret-value".to_owned())),
            badge: Some("B".to_owned()),
        };
        let shown = format!("{account:?}");
        assert!(!shown.contains("secret-value"), "{shown}");
        assert!(shown.contains(TOKEN_ENV) && shown.contains('B'), "{shown}");
    }

    #[test]
    fn management_errors_never_contain_the_token() {
        let secret = "secret token value";
        let (mut accounts, _) = Accounts::load(None);
        let mut errors = vec![accounts.add("B", secret).unwrap_err()];
        let dir = TempDir::new();
        let (mut accounts, _) = Accounts::load(Some(dir.0.clone()));
        errors.push(accounts.add("B", secret).unwrap_err());
        for err in errors {
            assert!(!err.to_string().contains("secret"), "{err}");
            assert!(!format!("{err:?}").contains("secret"), "{err:?}");
        }
    }

    #[test]
    fn pane_account_named_reads_that_token_without_selecting_it() {
        let dir = TempDir::new();
        dir.token("work.token", "tok-work\n", 0o600);
        let (accounts, skipped) = Accounts::load(Some(dir.0.clone()));
        assert_eq!(skipped.len(), 0);
        let work = accounts.pane_account_named("work").unwrap();
        assert_eq!(
            work.env,
            Some((TOKEN_ENV.to_owned(), "tok-work".to_owned()))
        );
        assert_eq!(work.badge.as_deref(), Some("work"));
        assert_eq!(
            accounts.pane_account_named(DEFAULT_ACCOUNT).unwrap(),
            PaneAccount::default()
        );
        assert!(matches!(
            accounts.pane_account_named("gone"),
            Err(AccountError::UnknownAccount { .. })
        ));
        assert_eq!(accounts.active_name(), DEFAULT_ACCOUNT);
    }
}
