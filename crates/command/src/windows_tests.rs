use std::os::windows::io::AsRawHandle as _;
use std::thread;
use std::time::Duration;

use super::JobObject;

#[test]
fn scoped_job_terminates_its_process_when_dropped() {
    let mut child = crate::blocking::Command::new("cmd.exe")
        .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
        .creation_flags(0)
        .spawn()
        .unwrap();
    let job = JobObject::new()
        .kill_children_on_close()
        .assign_process(child.as_raw_handle() as isize)
        .create_scoped()
        .unwrap();
    assert!(child.try_wait().unwrap().is_none());
    drop(job);
    for _ in 0..500 {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    panic!("Dropping the runtime job did not terminate its process");
}
