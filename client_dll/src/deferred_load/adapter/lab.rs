//! End-to-end own-host fixture: real cdecl callbacks, owned synthetic worlds.
//! Visual operations/epochs remain modeled, never engine calls.
use super::*;
use crate::deferred_load::{
    ActionKind, Phase,
    lab::{context, report, world},
};
use anyhow::{Result, ensure};
use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Normal,
    Nested,
    ArgumentsConflict,
    DescriptorConflict,
    ChangedBefore,
    ArgumentsAba,
    Cancelled,
    Expired,
}
const SCENARIOS: [(Scenario, &str); 8] = [
    (Scenario::Normal, "deferred_dispatch"),
    (Scenario::Nested, "nested_request_refused"),
    (Scenario::ArgumentsConflict, "published_arguments_retained"),
    (Scenario::DescriptorConflict, "foreign_descriptor_preserved"),
    (
        Scenario::ChangedBefore,
        "descriptor_changed_before_dispatch",
    ),
    (Scenario::ArgumentsAba, "arguments_aba_before_dispatch"),
    (Scenario::Cancelled, "cancelled_issued_dispatch"),
    (Scenario::Expired, "expired_issued_dispatch"),
];
struct Host {
    gate: Option<Gate>,
    slots: Arc<Arguments>,
    descriptor: Arc<Descriptor>,
    scenario: Scenario,
    callbacks: u64,
    refused: u64,
}
thread_local! { static HOST:RefCell<Option<Host>>=const{RefCell::new(None)}; }

// Only called with words originating in this fixture's live CString storage or
// Storage. No untrusted process addresses enter this fixture.
unsafe fn copy(frame: Frame) -> Option<OwnedCommand> {
    if !(2..=super::super::MAX_ARGUMENTS as u32).contains(&frame.argc) || frame.argv == 0 {
        return None;
    }
    let pointers = unsafe {
        std::slice::from_raw_parts(frame.argv as *const *const c_char, frame.argc as usize)
    };
    let tokens: Vec<_> = pointers
        .iter()
        .map(|p| unsafe { CStr::from_ptr(*p).to_bytes() })
        .collect();
    OwnedCommand::new(&tokens).ok()
}
extern "C" fn intercepted() -> u32 {
    HOST.with_borrow_mut(|host| {
        let Some(h) = host else { return 0 };
        // While replay owns the gate, even a nested map must not be forwarded.
        let Some(gate) = &mut h.gate else {
            h.refused += 1;
            return 0;
        };
        let Some(command) = (unsafe { copy(h.slots.snapshot().frame) }) else {
            h.refused += 1;
            return 0;
        };
        if gate
            .admit(context().owner_thread, command, world(1), true, 0, 100)
            .is_ok()
        {
            1
        } else {
            h.refused += 1;
            0
        }
    })
}
extern "C" fn foreign() -> u32 {
    7
}
extern "C" fn original() -> u32 {
    #[cfg(windows)]
    let incoming = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    let (slots, descriptor, scenario) = HOST.with_borrow_mut(|host| {
        let h = host.as_mut().unwrap();
        h.callbacks += 1;
        (Arc::clone(&h.slots), Arc::clone(&h.descriptor), h.scenario)
    });
    let frame = slots.snapshot().frame;
    let owned = unsafe { copy(frame) };
    let mut valid = owned.as_ref().is_some_and(|c| {
        c.argument(0) == Some(b"map".as_slice())
            && c.argument(1) == Some(b"fixture \"quoted\" \\ save".as_slice())
            && c.argument(2) == Some([0xe7].as_slice())
            && c.argument(3) == Some(b"".as_slice())
    });
    #[cfg(windows)]
    {
        valid &= incoming == 0x1234;
    }
    match scenario {
        Scenario::Nested => valid &= descriptor.call() == 0,
        Scenario::ArgumentsConflict => {
            // Keep argv published while changing argc: restoration must retain it.
            valid &= slots
                .replace(Frame {
                    argc: frame.argc + 1,
                    ..frame
                })
                .is_ok();
        }
        Scenario::DescriptorConflict => descriptor.replace(foreign),
        _ => (),
    }
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::Foundation::SetLastError(0xa55a)
    };
    u32::from(valid)
}
#[derive(Default, Debug, Serialize)]
pub struct Totals {
    pub scenarios: u64,
    pub intercepted: u64,
    pub callbacks: u64,
    pub nested_refusals: u64,
    pub quarantines: u64,
    pub retained_frames: u64,
    pub not_invoked: u64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub iterations: u32,
    pub totals: Totals,
    pub scenarios: Vec<&'static str>,
    pub own_callbacks: bool,
    pub visual_operations_simulated: bool,
    pub game_tested: bool,
    pub automatic_load_supported: bool,
}
struct Clear;
impl Drop for Clear {
    fn drop(&mut self) {
        HOST.set(None);
    }
}
fn one(scenario: Scenario, totals: &mut Totals) -> Result<()> {
    let source = [
        CString::new("map")?,
        CString::new("fixture \"quoted\" \\ save")?,
        CString::new([0xe7])?,
        CString::new("")?,
    ];
    let pointers: Vec<_> = source.iter().map(|s| s.as_ptr()).collect();
    let slots = Arguments::new(Frame {
        argc: 4,
        argv: pointers.as_ptr() as usize,
    });
    let descriptor = Descriptor::new(original);
    let mut hook = descriptor
        .install(original, intercepted)
        .map_err(|r| anyhow::anyhow!("{r:?}"))?;
    HOST.set(Some(Host {
        gate: Some(Gate::new(context().owner_thread)?),
        slots: Arc::clone(&slots),
        descriptor: Arc::clone(&descriptor),
        scenario,
        callbacks: 0,
        refused: 0,
    }));
    let _clear = Clear;
    ensure!(descriptor.call() == 1, "interception failed");
    ensure!(
        HOST.with_borrow(|h| h.as_ref().unwrap().callbacks) == 0,
        "load was not deferred"
    );
    // Simulate return from core's original stack frame; destroy all source bytes.
    slots
        .replace(Frame::default())
        .map_err(|r| anyhow::anyhow!("{r:?}"))?;
    drop(pointers);
    drop(source);
    let mut gate = HOST.with_borrow_mut(|h| h.as_mut().unwrap().gate.take().unwrap());
    let id = gate.snapshot().id.unwrap();
    for (now, kind, ack) in [
        (1, ActionKind::Hide, Ack::Hidden),
        (2, ActionKind::Check, Ack::Checked(report(true))),
        (3, ActionKind::Check, Ack::Checked(report(false))),
        (4, ActionKind::Release, Ack::Released),
    ] {
        let action = gate.poll(context(), world(1), now)?.unwrap();
        ensure!(action.kind == kind, "wrong preparation phase");
        gate.acknowledge(action.token, ack, now)?;
    }
    let action = gate.poll(context(), world(1), 5)?.unwrap();
    ensure!(action.kind == ActionKind::Execute, "missing dispatch");
    let expected = slots.snapshot();
    match scenario {
        Scenario::ChangedBefore => descriptor.replace(foreign),
        Scenario::ArgumentsAba => {
            slots.replace(Frame { argc: 99, argv: 0 }).unwrap();
            slots.replace(expected.frame).unwrap();
        }
        Scenario::Cancelled => gate.cancel(id)?,
        _ => (),
    }
    #[cfg(all(windows, target_arch = "x86"))]
    let before: u32;
    #[cfg(all(windows, target_arch = "x86"))]
    unsafe {
        core::arch::asm!("mov {0:e},esp",out(reg) before,options(nomem,nostack,preserves_flags));
    }
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::Foundation::SetLastError(0x1234)
    };
    let result = replay(
        &mut gate,
        &action,
        &hook,
        &slots,
        Dispatch {
            context: context(),
            world: world(1),
            now: if scenario == Scenario::Expired {
                100
            } else {
                5
            },
            arguments: expected,
        },
    );
    #[cfg(windows)]
    let outgoing = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    #[cfg(all(windows, target_arch = "x86"))]
    {
        let after: u32;
        unsafe {
            core::arch::asm!("mov {0:e},esp",out(reg) after,options(nomem,nostack,preserves_flags));
        }
        ensure!(before == after, "cdecl stack changed");
    }
    match scenario {
        Scenario::Cancelled | Scenario::Expired => {
            ensure!(result.is_err(), "cancelled/expired action executed");
            gate.acknowledge(
                action.token,
                Ack::NotInvoked,
                if scenario == Scenario::Expired {
                    100
                } else {
                    5
                },
            )?;
            totals.not_invoked += 1;
        }
        _ => {
            let r = result?;
            if matches!(scenario, Scenario::ChangedBefore | Scenario::ArgumentsAba) {
                ensure!(
                    !r.invoked && r.refusal.is_some(),
                    "preflight conflict dispatched"
                );
                totals.not_invoked += 1;
            } else {
                ensure!(
                    r.invoked && r.returned == Some(1),
                    "wrong original callback/arguments"
                );
                #[cfg(windows)]
                ensure!(outgoing == 0xa55a, "callback LastError not preserved");
            }
        }
    }
    let (calls, refused) = HOST.with_borrow(|h| {
        let h = h.as_ref().unwrap();
        (h.callbacks, h.refused)
    });
    totals.callbacks += calls;
    totals.nested_refusals += refused;
    ensure!(
        calls
            == u64::from(matches!(
                scenario,
                Scenario::Normal
                    | Scenario::Nested
                    | Scenario::ArgumentsConflict
                    | Scenario::DescriptorConflict
            )),
        "callback duplicated/lost"
    );
    if scenario == Scenario::Nested {
        ensure!(refused == 1, "nested request forwarded");
    }
    if scenario == Scenario::ArgumentsConflict {
        ensure!(slots.retained(), "published arguments were freed");
        ensure!(
            unsafe { slots.recover(slots.snapshot()) } == Err(Refusal::ArgumentsStillPublished),
            "published arguments reclaimed"
        );
        slots.replace(Frame::default()).unwrap();
        // All callbacks have returned; this fixture has no foreign readers.
        unsafe { slots.recover(slots.snapshot()) }.unwrap();
        ensure!(!slots.retained(), "explicit recovery failed");
        totals.retained_frames += 1;
    } else {
        ensure!(
            slots.snapshot().frame == Frame::default(),
            "previous argv not restored"
        );
    }
    if matches!(
        scenario,
        Scenario::ArgumentsConflict | Scenario::DescriptorConflict | Scenario::Cancelled
    ) {
        ensure!(
            gate.snapshot().phase == Phase::Quarantined,
            "uncertainty not quarantined"
        );
        gate.cancel(id)?;
        ensure!(
            gate.snapshot().phase == Phase::Quarantined,
            "cancel cleared quarantine"
        );
        ensure!(
            gate.poll(context(), world(2), 6)?.is_none(),
            "uncertain dispatch recreated"
        );
        totals.quarantines += 1;
        gate.recover(id, context(), world(2), true, true)?;
    } else if matches!(
        scenario,
        Scenario::ChangedBefore | Scenario::ArgumentsAba | Scenario::Expired
    ) {
        ensure!(
            gate.snapshot().phase == Phase::Recoverable,
            "known refusal not recoverable"
        );
        gate.recover(id, context(), world(1), true, true)?;
    } else {
        ensure!(
            gate.poll(context(), world(1), 6)?.is_none(),
            "callback return treated as load success"
        );
        let recreate = gate.poll(context(), world(2), 7)?.unwrap();
        ensure!(
            recreate.kind == ActionKind::Recreate,
            "new world not recreated"
        );
        gate.acknowledge(recreate.token, Ack::Recreated, 7)?;
        gate.finish(id)?;
    }
    if matches!(
        scenario,
        Scenario::DescriptorConflict | Scenario::ChangedBefore
    ) {
        ensure!(
            hook.restore() == Err(Refusal::DescriptorConflict),
            "foreign descriptor overwritten"
        );
        ensure!(descriptor.call() == 7, "foreign callback changed");
    } else {
        hook.restore().unwrap();
        hook.restore().unwrap();
    }
    ensure!(gate.snapshot().phase == Phase::Idle, "job leaked");
    totals.scenarios += 1;
    totals.intercepted += 1;
    Ok(())
}
pub fn run(iterations: u32) -> Result<Report> {
    ensure!(
        (1..=10000).contains(&iterations),
        "iterations must be 1..=10000"
    );
    ensure!(
        HOST.with_borrow(Option::is_none),
        "fixture already active on this thread"
    );
    let mut totals = Totals::default();
    for _ in 0..iterations {
        for (scenario, _) in SCENARIOS {
            one(scenario, &mut totals)?;
        }
    }
    Ok(Report {
        iterations,
        totals,
        scenarios: SCENARIOS.iter().map(|(_, name)| *name).collect(),
        own_callbacks: true,
        visual_operations_simulated: true,
        game_tested: false,
        automatic_load_supported: false,
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn actual_fixture_descriptor_drives_gate_dispatch_and_conflict_recovery() {
        let r = super::run(25).unwrap();
        assert_eq!(r.totals.scenarios, 200);
        assert_eq!(r.totals.callbacks, 100);
        assert_eq!(r.totals.quarantines, 75);
        assert_eq!(r.totals.not_invoked, 100);
        assert_eq!(r.totals.nested_refusals, 25);
        assert_eq!(r.totals.retained_frames, 25);
    }
}
