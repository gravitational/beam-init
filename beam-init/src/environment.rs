use std::collections::BTreeMap;
use std::ffi::{CStr, OsString};
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::{io, ptr};

const DEFAULT_USER_PATH: &str = "/usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games";
const DEFAULT_ROOT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Builds the base environment including HOME, USER, and PATH along with all variables specified by
/// the env_files.
pub fn build_environment<P: AsRef<Path>>(
    user: libc::uid_t,
    env_files: &[P],
) -> io::Result<BTreeMap<OsString, OsString>> {
    let mut env = build_base_environment(user);
    if user == 0 {
        env.insert("PATH".into(), DEFAULT_ROOT_PATH.into());
    } else {
        env.insert("PATH".into(), DEFAULT_USER_PATH.into());
    }
    let envf = env_from_files(env_files)?;
    env.extend(envf.into_iter().map(|(k, v)| (k.into(), v.into())));

    Ok(env)
}

fn env_from_file<P: AsRef<Path>>(env_file: P) -> io::Result<BTreeMap<String, String>> {
    let contents = std::fs::read_to_string(env_file)?;
    Ok(contents
        .lines()
        .filter_map(env_line)
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect())
}

fn env_from_files<P: AsRef<Path>>(env_files: &[P]) -> io::Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::default();
    for env_file in env_files {
        let contents = env_from_file(env_file)?;
        env.extend(contents);
    }
    Ok(env)
}

// Conservative line validation. Rejects keys that are not alphanumeric.
// Inline comments are not handled and added as part of the value.
// Users control the files passed in so this feels acceptable vs trying to 100%
// compatible Linux-PAM.
fn env_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (k, mut v): (&str, &str) = line.split_once('=')?;
    if k.is_empty() || !k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
        return None;
    }
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v = &v[1..(v.len() - 1)]
    }
    Some((k, v))
}

// Base env will contain HOME, USER, and PATH by default
fn build_base_environment(user: libc::uid_t) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::default();
    match get_user_info(user) {
        Ok(Some(user_info)) => {
            env.insert("USER".into(), user_info.user);
            env.insert("HOME".into(), user_info.home);
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("failed to build base environment: {e}");
        }
    }
    env
}

struct UserInfo {
    user: OsString,
    home: OsString,
}

// Returns the user information from a given uid
fn get_user_info(user: libc::uid_t) -> io::Result<Option<UserInfo>> {
    // SAFETY: sysconf is safe to call
    let mut buflen = unsafe {
        let ret = libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX);
        if ret == -1 { 1024 } else { ret }
    } as usize;

    let mut buf: Vec<u8> = vec![0u8; buflen];

    // getpwuid_r will return ERANGE if the buffer is not big enough so loop until we have a buffer
    // big enough
    let pwd = loop {
        let mut pwd = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();

        // SAFETY:
        // - pwd matches expected type
        // - pwd and result writable
        // - buf remains alive while pointers in pwd are used
        match unsafe {
            libc::getpwuid_r(
                user,
                pwd.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        } {
            libc::ERANGE => {
                buflen = buflen
                    .checked_mul(2)
                    .ok_or_else(|| io::Error::other("buffer too large"))?;
                buf.resize(buflen, 0);
            }
            // User not found
            0 if result.is_null() => return Ok(None),
            0 => {
                // SAFETY: no error returned and result was not null
                break unsafe { pwd.assume_init() };
            }
            error => return Err(io::Error::from_raw_os_error(error)),
        }
    };

    let user = if pwd.pw_name.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "user is null"));
    } else {
        // SAFETY: name is not null and buf is still alive
        let user = unsafe { CStr::from_ptr(pwd.pw_name) };
        OsString::from_vec(user.to_bytes().to_vec())
    };

    let home = if pwd.pw_dir.is_null() {
        OsString::new()
    } else {
        // SAFETY: home is not null and buf is still alive
        let home = unsafe { CStr::from_ptr(pwd.pw_dir) };
        OsString::from_vec(home.to_bytes().to_vec())
    };

    Ok(Some(UserInfo { user, home }))
}

#[cfg(test)]
mod tests {
    use super::env_line;

    #[test]
    fn env_line_check() {
        assert_eq!(env_line("# comment"), None);
        assert_eq!(env_line("key"), None);
        assert_eq!(env_line("bad key=value"), None);

        assert_eq!(env_line("key=value"), Some(("key", "value")));
        assert_eq!(env_line("key=\"value\""), Some(("key", "value")));
        assert_eq!(env_line("key="), Some(("key", "")));
        assert_eq!(env_line("key=\"\""), Some(("key", "")));
        assert_eq!(env_line("key=''"), Some(("key", "")));
        assert_eq!(env_line("key=a=b"), Some(("key", "a=b")));
    }
}
