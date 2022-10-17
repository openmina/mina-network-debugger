fn main() {
    use std::{
        process::{Command, Stdio},
        thread,
        time::Duration,
        io::Write,
    };
    use pete::{Ptracer, Stop, Restart};

    let mut tracer = Ptracer::new();
    let mut cat = Command::new("cat");
    cat.stdin(Stdio::piped()).stdout(Stdio::null());
    let mut cat = tracer.spawn(cat).unwrap();
    let mut pipe = cat.stdin.take().unwrap();
    thread::spawn(move || loop {
        let tracee = tracer.wait().unwrap().unwrap();
        match &tracee.stop {
            Stop::SyscallEnter => {
                let regs = tracee.registers().unwrap();
                println!("enter orig_rax: {} rax: {}", regs.orig_rax, regs.rax as i64);
            }
            Stop::SyscallExit => {
                let regs = tracee.registers().unwrap();
                println!("exit orig_rax: {} rax: {}", regs.orig_rax, regs.rax as i64);
            }
            Stop::Exiting { exit_code } => {
                println!("exitting: {exit_code}");
                break;
            }
            _ => (),
        }
        tracer.restart(tracee, Restart::Syscall).unwrap();
    });
    loop {
        thread::sleep(Duration::from_secs(2));
        pipe.write_all(b"hello").unwrap();
    }
}
