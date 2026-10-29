fn main() {
    let executable = std::env::current_exe().expect("locate the fake hgs launcher");
    let script = executable
        .parent()
        .expect("the fake hgs launcher has a folder")
        .join("hgs");
    let status = std::process::Command::new("python")
        .arg(script)
        .args(std::env::args_os().skip(1))
        .status()
        .expect("run the fake hgs with Python");
    std::process::exit(status.code().unwrap_or(1));
}
