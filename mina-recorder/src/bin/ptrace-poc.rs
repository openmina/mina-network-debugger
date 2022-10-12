fn main() {
    use std::process::Command;
    use pete::{Ptracer, Stop, Restart};

    let mut tracer = Ptracer::new();
    tracer.spawn(Command::new("ls")).unwrap();
    loop {
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
    }
}
