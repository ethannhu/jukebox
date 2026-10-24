use std::{
    fs::{self, OpenOptions},
    io,
    path::PathBuf,
};

fn session_file() -> io::Result<PathBuf> {
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "找不到 XDG_CONFIG_HOME 或 HOME"))?;
    Ok(config_dir.join("ncm-rs").join("session.cookies"))
}

pub fn load_cookie() -> io::Result<Option<String>> {
    match fs::read_to_string(session_file()?) {
        Ok(cookie) if !cookie.trim().is_empty() => Ok(Some(cookie.trim().to_string())),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn save_cookie(cookie: &str) -> io::Result<()> {
    let path = session_file()?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "无效的会话文件路径"))?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }

    let tmp_path = path.with_extension("cookies.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp_path)?;
    use std::io::Write;
    file.write_all(cookie.as_bytes())?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(tmp_path, path)?;
    Ok(())
}

pub fn clear_cookie() -> io::Result<()> {
    match fs::remove_file(session_file()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
