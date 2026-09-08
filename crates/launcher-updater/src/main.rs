use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            let mut last = None;
            for _ in 0..80 {
                match fs::copy(entry.path(), &destination) {
                    Ok(_) => {
                        last = None;
                        break;
                    }
                    Err(error) => {
                        last = Some(error);
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            if let Some(error) = last {
                return Err(error);
            }
        }
    }
    Ok(())
}

fn process_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err("usage: updater <pid> <source> <target>".into());
    }
    let pid: u32 = args[1].parse()?;
    let source = PathBuf::from(&args[2]).canonicalize()?;
    let target = PathBuf::from(&args[3]).canonicalize()?;
    if !source.join("ck-launcher-qt.exe").is_file()
        || !source.join("ck-launcher-service.exe").is_file()
    {
        return Err("incomplete update package".into());
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while process_alive(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(200));
    }
    copy_tree(&source, &target)?;
    Command::new(target.join("ck-launcher-qt.exe"))
        .current_dir(&target)
        .spawn()?;
    let _ = fs::remove_dir_all(source.parent().unwrap_or(&source));
    Ok(())
}
