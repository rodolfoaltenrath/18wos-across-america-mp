//! Own cdecl callback/argument frame only. Does not load any game image or DLL.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    deferred_load::{Ack, ActionKind, Gate, OwnedCommand, Phase, World},
    release_policy::{self, Context, Evidence},
};
use std::{
    cell::Cell,
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        Threading::{GetCurrentProcess, GetCurrentThreadId},
    },
};
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    argc: u32,
    argv: *const *const c_char,
}
thread_local! {
    static FRAME:Cell<Frame>=const{Cell::new(Frame{argc:0,argv:ptr::null()})};
    static CALLS:Cell<u32>=const{Cell::new(0)};
}
struct Restore(Frame);
impl Drop for Restore {
    fn drop(&mut self) {
        FRAME.set(self.0);
    }
}
fn bound<R>(command: &OwnedCommand, f: impl FnOnce() -> R) -> R {
    let incoming = unsafe { GetLastError() };
    let (result, error) = command.with_argv(|argc, argv| {
        let _restore = Restore(FRAME.replace(Frame { argc, argv }));
        unsafe { SetLastError(incoming) };
        let result = f();
        (result, unsafe { GetLastError() })
    });
    unsafe { SetLastError(error) };
    result
}
unsafe extern "C" fn callback() -> u32 {
    let incoming = unsafe { GetLastError() };
    CALLS.set(CALLS.get() + 1);
    let f = FRAME.get();
    let valid = if incoming == 0x1234 && f.argc == 3 && !f.argv.is_null() {
        let argv = unsafe { std::slice::from_raw_parts(f.argv, 3) };
        unsafe {
            CStr::from_ptr(argv[0]).to_bytes() == b"map"
                && CStr::from_ptr(argv[1]).to_bytes() == b"fixture \"quoted\" \\ save"
                && CStr::from_ptr(argv[2]).to_bytes() == [0xe7]
        }
    } else {
        false
    };
    unsafe { SetLastError(0xa55a) };
    u32::from(valid)
}
fn context() -> Context {
    let thread = unsafe { GetCurrentThreadId() };
    Context {
        current_thread: thread,
        owner_thread: thread,
        pump_installed: true,
        pump_depth: 1,
        ..Context::default()
    }
}
fn world(epoch: u64) -> World {
    World {
        epoch,
        scene: 10,
        actor: 20,
        ready: true,
    }
}
fn capture() -> OwnedCommand {
    let strings = [
        CString::new("map").unwrap(),
        CString::new("fixture \"quoted\" \\ save").unwrap(),
        CString::new([0xe7]).unwrap(),
    ];
    let argv: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
    OwnedCommand::capture(3, argv.as_ptr() as u32, |at, out| {
        let mut done = 0;
        if unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                at as usize as _,
                out.as_mut_ptr().cast(),
                out.len(),
                &mut done,
            )
        } == 0
            || done != out.len()
        {
            return Err("fixture read failed".into());
        }
        Ok(())
    })
    .unwrap()
    // The original pointer array/strings are destroyed here.
}
#[test]
fn cdecl_callback_runs_once_after_release_with_owned_args_and_restores_frame_and_error() {
    let c = context();
    let mut gate = Gate::new(c.owner_thread).unwrap();
    let id = gate
        .admit(c.owner_thread, capture(), world(1), true, 0, 100)
        .unwrap();
    let mut action = gate.poll(c, world(1), 1).unwrap().unwrap();
    assert_eq!(action.kind, ActionKind::Hide);
    gate.acknowledge(action.token, Ack::Hidden, 1).unwrap();
    let evidence = Evidence {
        allocated: true,
        item: 99,
        drawn: 1,
        ever_drawn: true,
        flushes: 20,
        last_draw_flush: 1,
        hidden_at: 2,
        counters: Some([1, 1, 1]),
        bindings_present: true,
        ..Evidence::default()
    };
    action = gate.poll(c, world(1), 2).unwrap().unwrap();
    gate.acknowledge(
        action.token,
        Ack::Checked(release_policy::assess(c, evidence)),
        2,
    )
    .unwrap();
    assert_eq!(CALLS.get(), 0);
    action = gate.poll(c, world(1), 3).unwrap().unwrap();
    gate.acknowledge(
        action.token,
        Ack::Checked(release_policy::assess(
            c,
            Evidence {
                counters: Some([1, 1, 0]),
                ..evidence
            },
        )),
        3,
    )
    .unwrap();
    action = gate.poll(c, world(1), 4).unwrap().unwrap();
    assert_eq!(action.kind, ActionKind::Release);
    gate.acknowledge(action.token, Ack::Released, 4).unwrap();
    assert_eq!(CALLS.get(), 0);
    action = gate.poll(c, world(1), 5).unwrap().unwrap();
    assert_eq!(action.kind, ActionKind::Execute);
    let old = FRAME.get();
    unsafe { SetLastError(0x1234) };
    let before: u32;
    unsafe {
        core::arch::asm!("mov {0:e},esp",out(reg) before,options(nomem,nostack,preserves_flags));
    }
    let execute: unsafe extern "C" fn() -> u32 = callback;
    let result = bound(action.command().unwrap(), || unsafe { execute() });
    // TLS helpers themselves may call TlsGetValue, which changes LastError.
    let returned_error = unsafe { GetLastError() };
    let after: u32;
    unsafe {
        core::arch::asm!("mov {0:e},esp",out(reg) after,options(nomem,nostack,preserves_flags));
    }
    assert_eq!(before, after);
    assert_eq!(result, 1);
    assert_eq!(CALLS.get(), 1);
    assert_eq!(FRAME.get(), old);
    assert_eq!(returned_error, 0xa55a);
    gate.acknowledge(action.token, Ack::ExecuteReturned, 5)
        .unwrap();
    assert!(gate.poll(c, world(1), 6).unwrap().is_none());
    let recreate = gate.poll(c, world(2), 7).unwrap().unwrap();
    assert_eq!(recreate.kind, ActionKind::Recreate);
    gate.acknowledge(recreate.token, Ack::Recreated, 7).unwrap();
    assert_eq!(
        gate.acknowledge(action.token, Ack::ExecuteReturned, 7),
        Err(client_dll::deferred_load::Error::StaleAction)
    );
    gate.finish(id).unwrap();
    assert_eq!(gate.snapshot().phase, Phase::Idle);
    assert_eq!(CALLS.get(), 1);
}
#[test]
fn argument_frame_is_restored_on_rust_unwind_without_invoking_a_callback() {
    let command = capture();
    let old = FRAME.get();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        bound(&command, || {
            assert_eq!(FRAME.get().argc, 3);
            panic!("fixture interruption before callback");
        })
    }));
    assert!(outcome.is_err());
    assert_eq!(FRAME.get(), old);
    assert_eq!(CALLS.get(), 0);
    unsafe { SetLastError(0x1234) };
    assert_eq!(bound(&command, || unsafe { callback() }), 1);
    assert_eq!(FRAME.get(), old);
}
