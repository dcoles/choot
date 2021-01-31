use std::env::args;
use std::ffi::CString;
use std::process::exit;
use std::path::{Path, PathBuf};

use nix::mount::{mount, MsFlags};
use nix::sched::unshare;
use nix::sched::CloneFlags;
use nix::sys::wait::{wait, WaitStatus};
use nix::sys::stat::{mknod, makedev, Mode, SFlag};
use nix::unistd::{chdir, chroot, execv, fork, geteuid, symlinkat, ForkResult};


fn main() {
    if !geteuid().is_root() {
        eprintln!("Must be run as root");
        exit(1);
    }

    let args: Vec<String> = args().collect();
    if args.len() != 2 {
        eprintln!("usage: {} ROOT", args[0]);
        exit(2);
    }

    let target = PathBuf::from(args[1].as_str());

    let flags = CloneFlags::CLONE_NEWNS
        | CloneFlags::CLONE_NEWPID
        | CloneFlags::CLONE_NEWIPC
        | CloneFlags::CLONE_NEWUTS;
    unshare(flags).expect("Unshare failed");

    match unsafe { fork().expect("Fork failed") } {
        ForkResult::Parent { .. } => {
            // Wait for child to exit
            match wait().expect("Wait failed") {
                WaitStatus::Exited(_, exitcode) => exit(exitcode),
                _ => exit(1),
            }
        },
        ForkResult::Child => (),
    }

    let root = PathBuf::from("/");
    make_rslave(&root).expect("Failed to mark root rslave");
    bind_mount(&target, &target).expect("Failed to bind-mount root");

    // Mounts
    mount_special(target.join("proc"), "proc", MsFlags::empty(), None).expect("Failed to mount proc");
    mount_special(target.join("sys"), "sysfs", MsFlags::empty(), None).expect("Failed to mount sysfs");
    mount_special(target.join("dev"), "tmpfs", MsFlags::MS_NOSUID | MsFlags::MS_STRICTATIME, Some("mode=755")).expect("Failed to mount dev tmpfs");

    // Devices
    let dev_mode: Mode = Mode::S_IRUSR | Mode::S_IWUSR | Mode::S_IRGRP | Mode::S_IWGRP | Mode::S_IROTH | Mode::S_IWOTH;
    make_chardev(target.join("dev/null"), dev_mode, 1, 3).expect("Failed to make /dev/null");
    make_chardev(target.join("dev/zero"), dev_mode, 1, 5).expect("Failed to make /dev/zero");
    make_chardev(target.join("dev/full"), dev_mode, 1, 7).expect("Failed to make /dev/full");
    make_chardev(target.join("dev/random"), dev_mode, 1, 8).expect("Failed to make /dev/random");
    make_chardev(target.join("dev/urandom"), dev_mode, 1, 9).expect("Failed to make /dev/urandom");
    make_chardev(target.join("dev/tty"), dev_mode, 5, 0).expect("Failed to make /dev/tty");
    make_chardev(target.join("dev/ptmx"), dev_mode, 5, 2).expect("Failed to make /dev/ptmx");

    symlink("/proc/self/fd", target.join("dev/fd")).expect("Failed to symlink /dev/fd");
    symlink("/proc/self/fd/0", target.join("dev/stdin")).expect("Failed to symlink /dev/stdin");
    symlink("/proc/self/fd/1", target.join("dev/stdout")).expect("Failed to symlink /dev/stdout");
    symlink("/proc/self/fd/2", target.join("dev/stderr")).expect("Failed to symlink /dev/stderr");

    // Enter chroot
    chdir(&target).expect("Failed to chdir to target");
    move_mount(".", root).expect("Failed to move root");
    chroot(".").expect("Failed to chroot to target");

    let shell = CString::new("/bin/sh").unwrap();
    execv(&shell, &[&shell]).unwrap();
}

fn make_rslave<T: AsRef<Path>>(target: T) -> nix::Result<()> {
    mount(None::<&Path>, target.as_ref(), None::<&Path>, MsFlags::MS_REC | MsFlags::MS_SLAVE, None::<&Path>)
}

fn bind_mount<T: AsRef<Path>, U: AsRef<Path>>(source: T, target: U) -> nix::Result<()> {
    mount(Some(source.as_ref()), target.as_ref(), None::<&Path>, MsFlags::MS_BIND, None::<&Path>)
}

fn mount_special<T: AsRef<Path>>(target: T, fstype: &str, flags: MsFlags, data: Option<&str>) -> nix::Result<()> {
    mount(Some(fstype), target.as_ref(), Some(fstype), flags, data)
}

fn move_mount<T: AsRef<Path>, U: AsRef<Path>>(source: T, target: U) -> nix::Result<()> {
    mount(Some(source.as_ref()), target.as_ref(), None::<&Path>, MsFlags::MS_MOVE, None::<&Path>)
}

fn make_chardev<T: AsRef<Path>>(target: T, mode: Mode, major: u64, minor: u64) -> nix::Result<()> {
    mknod(target.as_ref(), SFlag::S_IFCHR, mode, makedev(major, minor))
}

fn symlink<T: AsRef<Path>, U: AsRef<Path>>(target: T, linkpath: U) -> nix::Result<()> {
    symlinkat(target.as_ref(), None, linkpath.as_ref())
}
