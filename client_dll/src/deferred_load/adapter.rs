//! Concrete descriptor/argv transactions for an OWN in-process host only.
//!
//! There is deliberately no address-based constructor, game binding, DLL hook,
//! lifecycle inference or SEH recovery. The fixture serializes word updates;
//! this does not establish quiescence of the game's uncoordinated globals.
use super::{Ack, Action, Gate, OwnedCommand, World};
use crate::release_policy::Context;
use serde::Serialize;
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicPtr, Ordering},
    },
};

pub mod lab;
/// All fixture callbacks have static code lifetime and must return normally.
pub type Callback = extern "C" fn() -> u32;

/// Own map descriptor, not a pointer into an executable image. On x86 this is
/// the profiled 16-byte shape: name/callback/hash/next, callback at byte four.
#[repr(C)]
pub struct Descriptor {
    name: usize,
    callback: AtomicPtr<()>,
    hash: u32,
    next: usize,
}
impl Descriptor {
    pub fn new(callback: Callback) -> Arc<Self> {
        Arc::new(Self {
            name: c"map".as_ptr() as usize,
            callback: AtomicPtr::new(callback as *mut ()),
            hash: 318,
            next: 0,
        })
    }
    pub fn call(&self) -> u32 {
        // Every writer accepts a typed, static callback; never a raw address.
        let callback: Callback =
            unsafe { std::mem::transmute(self.callback.load(Ordering::SeqCst)) };
        callback()
    }
    /// Models another descriptor owner. Does not deregister or free code.
    pub fn replace(&self, callback: Callback) {
        self.callback.store(callback as *mut (), Ordering::SeqCst);
    }
    pub fn install(
        self: &Arc<Self>,
        original: Callback,
        replacement: Callback,
    ) -> Result<Hook, Refusal> {
        if std::ptr::fn_addr_eq(original, replacement) {
            return Err(Refusal::SameCallback);
        }
        self.callback
            .compare_exchange(
                original as *mut (),
                replacement as *mut (),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map_err(|_| Refusal::DescriptorConflict)?;
        Ok(Hook {
            descriptor: Arc::clone(self),
            original,
            replacement,
            installed: true,
        })
    }
}
pub struct Hook {
    descriptor: Arc<Descriptor>,
    original: Callback,
    replacement: Callback,
    installed: bool,
}
impl Hook {
    pub fn owns(&self) -> bool {
        self.installed
            && self.descriptor.callback.load(Ordering::SeqCst) == self.replacement as *mut ()
    }
    /// CAS never overwrites a replacement installed by another owner.
    pub fn restore(&mut self) -> Result<(), Refusal> {
        if !self.installed {
            return Ok(());
        }
        self.descriptor
            .callback
            .compare_exchange(
                self.replacement as *mut (),
                self.original as *mut (),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map_err(|_| Refusal::DescriptorConflict)?;
        self.installed = false;
        Ok(())
    }
}
impl Drop for Hook {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub argc: u32,
    pub argv: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub frame: Frame,
    revision: u64,
}
struct Storage {
    _command: Arc<OwnedCommand>,
    pointers: Box<[usize]>,
}
impl Storage {
    fn new(command: Arc<OwnedCommand>) -> Self {
        let pointers = command.with_argv(|argc, argv| {
            // The source array is temporary; its CString bytes belong to command.
            unsafe { std::slice::from_raw_parts(argv, argc as usize) }
                .iter()
                .map(|p| *p as usize)
                .collect::<Vec<_>>()
                .into_boxed_slice()
        });
        Self {
            _command: command,
            pointers,
        }
    }
    fn frame(&self) -> Frame {
        Frame {
            argc: self.pointers.len() as u32,
            argv: self.pointers.as_ptr() as usize,
        }
    }
}
struct Words {
    frame: Frame,
    revision: u64,
    active: Option<u64>,
    retained: Option<Storage>,
}
/// Shared argc/argv words of the own fixture. Callbacks run without its mutex.
pub struct Arguments(Mutex<Words>);
impl Arguments {
    pub fn new(frame: Frame) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Words {
            frame,
            revision: 0,
            active: None,
            retained: None,
        })))
    }
    pub fn snapshot(&self) -> Snapshot {
        let s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        Snapshot {
            frame: s.frame,
            revision: s.revision,
        }
    }
    /// Models a nested command or a foreign writer, including ABA changes.
    pub fn replace(&self, frame: Frame) -> Result<(), Refusal> {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        s.revision = s.revision.checked_add(1).ok_or(Refusal::Exhausted)?;
        s.frame = frame;
        Ok(())
    }
    pub fn retained(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retained
            .is_some()
    }
    pub fn bind(
        self: &Arc<Self>,
        command: Arc<OwnedCommand>,
        expected: Snapshot,
    ) -> Result<Binding, Refusal> {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if s.active.is_some() || s.retained.is_some() {
            return Err(Refusal::ArgumentsBusy);
        }
        if s.frame != expected.frame || s.revision != expected.revision {
            return Err(Refusal::ArgumentsChanged);
        }
        // Reserve one revision for binding and one for restoration.
        let revision = s.revision.checked_add(2).ok_or(Refusal::Exhausted)? - 1;
        let storage = Storage::new(command);
        s.frame = storage.frame();
        s.revision = revision;
        s.active = Some(revision);
        Ok(Binding {
            slots: Arc::clone(self),
            old: expected.frame,
            revision,
            storage: Some(storage),
            // Bind/unbind must stay on one fixture owner thread.
            _not_send: PhantomData,
        })
    }
    /// Reclaim a conflict's retained arguments, after explicit external proof.
    ///
    /// # Safety
    /// No callback, nested command or foreign reader may retain any pointer to
    /// the quarantined array/strings. That proof is supplied by the own host;
    /// a boolean or changed argv value alone cannot prove it for the game.
    pub unsafe fn recover(&self, expected: Snapshot) -> Result<(), Refusal> {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if s.active.is_some() {
            return Err(Refusal::ArgumentsBusy);
        }
        if s.frame != expected.frame || s.revision != expected.revision {
            return Err(Refusal::ArgumentsChanged);
        }
        if s.retained
            .as_ref()
            .is_some_and(|r| r.frame().argv == s.frame.argv)
        {
            return Err(Refusal::ArgumentsStillPublished);
        }
        s.retained = None;
        Ok(())
    }
}
impl Drop for Arguments {
    fn drop(&mut self) {
        // Unproven foreign references must not become dangling on host drop.
        if let Some(storage) = self
            .0
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .retained
            .take()
        {
            std::mem::forget(storage);
        }
    }
}
pub struct Binding {
    slots: Arc<Arguments>,
    old: Frame,
    revision: u64,
    storage: Option<Storage>,
    _not_send: PhantomData<Rc<()>>,
}
impl Binding {
    pub fn finish(mut self) -> bool {
        self.restore()
    }
    fn restore(&mut self) -> bool {
        let Some(storage) = self.storage.take() else {
            return true;
        };
        let mut s = self.slots.0.lock().unwrap_or_else(|p| p.into_inner());
        let owned = s.active == Some(self.revision)
            && s.revision == self.revision
            && s.frame == storage.frame();
        s.active = None;
        if owned {
            s.frame = self.old;
            s.revision += 1; // Reserved by bind; a foreign write fails owned above.
        } else {
            // At most one retained frame; bind refuses while it is retained.
            s.retained = Some(storage);
        }
        owned
    }
}
impl Drop for Binding {
    fn drop(&mut self) {
        self.restore();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    SameCallback,
    DescriptorConflict,
    ArgumentsChanged,
    ArgumentsBusy,
    ArgumentsStillPublished,
    Exhausted,
}
#[derive(Debug, Serialize)]
pub struct ReplayReport {
    pub invoked: bool,
    pub returned: Option<u32>,
    pub arguments_restored: bool,
    pub descriptor_owned: bool,
    pub refusal: Option<Refusal>,
    pub game_tested: bool,
    pub automatic_load_supported: bool,
}
pub struct Dispatch {
    pub context: Context,
    pub world: World,
    pub now: u64,
    pub arguments: Snapshot,
}
struct ErrorScope {
    #[cfg(windows)]
    value: u32,
}
impl ErrorScope {
    fn capture() -> Self {
        Self {
            #[cfg(windows)]
            value: unsafe { windows_sys::Win32::Foundation::GetLastError() },
        }
    }
    fn enter_callback(&self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::SetLastError(self.value)
        };
    }
    fn leave_callback(&mut self) {
        #[cfg(windows)]
        {
            self.value = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        }
    }
}
impl Drop for ErrorScope {
    fn drop(&mut self) {
        self.enter_callback();
    }
}
/// Dispatch the ORIGINAL fixture callback exactly once, bypassing its hook.
/// Revalidation errors leave the action pending for owner-thread resolution.
/// Binding/hook refusals acknowledge NotInvoked; a post-call conflict acknowledges
/// Unknown, preserving arguments and preventing automatic replay/recreation.
pub fn replay(
    gate: &mut Gate,
    action: &Action,
    hook: &Hook,
    slots: &Arc<Arguments>,
    dispatch: Dispatch,
) -> Result<ReplayReport, super::Error> {
    let mut error = ErrorScope::capture();
    gate.validate_execute(action, dispatch.context, dispatch.world, dispatch.now)?;
    let mut report = ReplayReport {
        invoked: false,
        returned: None,
        arguments_restored: false,
        descriptor_owned: hook.owns(),
        refusal: None,
        game_tested: false,
        automatic_load_supported: false,
    };
    let mut uncertain = false;
    if !report.descriptor_owned {
        report.refusal = Some(Refusal::DescriptorConflict);
    } else {
        match slots.bind(
            Arc::clone(action.command.as_ref().unwrap()),
            dispatch.arguments,
        ) {
            Err(reason) => report.refusal = Some(reason),
            Ok(binding) => {
                // Recheck descriptor after preparing the argument array.
                if !hook.owns() {
                    report.arguments_restored = binding.finish();
                    uncertain = !report.arguments_restored;
                    report.descriptor_owned = false;
                    report.refusal = Some(Refusal::DescriptorConflict);
                } else {
                    error.enter_callback();
                    let returned = (hook.original)();
                    error.leave_callback();
                    report.invoked = true;
                    report.returned = Some(returned);
                    report.arguments_restored = binding.finish();
                    report.descriptor_owned = hook.owns();
                    if !report.arguments_restored {
                        report.refusal = Some(Refusal::ArgumentsChanged);
                    } else if !report.descriptor_owned {
                        report.refusal = Some(Refusal::DescriptorConflict);
                    }
                    gate.acknowledge(
                        action.token,
                        if report.refusal.is_none() {
                            Ack::ExecuteReturned
                        } else {
                            Ack::Unknown
                        },
                        dispatch.now,
                    )?;
                    return Ok(report);
                }
            }
        }
    }
    gate.acknowledge(
        action.token,
        if uncertain {
            Ack::Unknown
        } else {
            Ack::NotInvoked
        },
        dispatch.now,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::{
        ActionKind, Error, Phase,
        lab::{command, context, report, world},
    };
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    extern "C" fn original() -> u32 {
        11
    }
    extern "C" fn replacement() -> u32 {
        22
    }
    extern "C" fn foreign() -> u32 {
        33
    }
    #[test]
    fn descriptor_header_and_argument_words_match_x86_offsets() {
        assert_eq!(
            std::mem::offset_of!(Descriptor, callback),
            std::mem::size_of::<usize>()
        );
        #[cfg(target_pointer_width = "32")]
        {
            assert_eq!(std::mem::size_of::<Descriptor>(), 16);
            assert_eq!(std::mem::offset_of!(Descriptor, callback), 4);
            assert_eq!(std::mem::offset_of!(Descriptor, hash), 8);
            assert_eq!(std::mem::offset_of!(Descriptor, next), 12);
            assert_eq!(std::mem::size_of::<Frame>(), 8);
            assert_eq!(std::mem::offset_of!(Frame, argv), 4);
        }
        let d = Descriptor::new(original);
        let header = (d.name, d.hash, d.next);
        let mut hook = d.install(original, replacement).unwrap();
        assert_eq!((d.name, d.hash, d.next), header);
        hook.restore().unwrap();
        assert_eq!((d.name, d.hash, d.next), header);
        assert_eq!(header.1, 318);
    }
    fn ready() -> (Gate, Action) {
        let mut gate = Gate::new(context().owner_thread).unwrap();
        gate.admit(context().owner_thread, command(), world(1), true, 0, 100)
            .unwrap();
        for (now, kind, ack) in [
            (1, ActionKind::Hide, Ack::Hidden),
            (2, ActionKind::Check, Ack::Checked(report(false))),
            (3, ActionKind::Release, Ack::Released),
        ] {
            let a = gate.poll(context(), world(1), now).unwrap().unwrap();
            assert_eq!(a.kind, kind);
            gate.acknowledge(a.token, ack, now).unwrap();
        }
        let action = gate.poll(context(), world(1), 4).unwrap().unwrap();
        (gate, action)
    }
    #[test]
    fn descriptor_lease_compares_both_install_and_restore_and_drop_is_conditional() {
        let d = Descriptor::new(original);
        assert!(matches!(
            d.install(original, original),
            Err(Refusal::SameCallback)
        ));
        let mut h = d.install(original, replacement).unwrap();
        assert_eq!(d.call(), 22);
        assert!(matches!(
            d.install(original, foreign),
            Err(Refusal::DescriptorConflict)
        ));
        d.replace(foreign);
        assert!(!h.owns());
        assert_eq!(h.restore(), Err(Refusal::DescriptorConflict));
        drop(h);
        assert_eq!(d.call(), 33);
        let h = d.install(foreign, replacement).unwrap();
        drop(h);
        assert_eq!(d.call(), 33);
    }
    #[test]
    fn binding_owns_bytes_excludes_nesting_and_restores_on_rust_unwind() {
        let old = Frame { argc: 7, argv: 99 };
        let slots = Arguments::new(old);
        let command = Arc::new(command());
        let weak = Arc::downgrade(&command);
        let before = slots.snapshot();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _binding = slots.bind(command, before).unwrap();
            assert!(weak.upgrade().is_some());
            assert!(matches!(
                slots.bind(Arc::new(super::super::lab::command()), slots.snapshot()),
                Err(Refusal::ArgumentsBusy)
            ));
            panic!("own interruption before callback");
        }));
        assert!(result.is_err());
        assert_eq!(slots.snapshot().frame, old);
        assert!(slots.snapshot().revision > before.revision);
        assert!(!slots.retained());
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn snapshot_detects_aba_before_binding_and_revision_exhaustion() {
        let slots = Arguments::new(Frame::default());
        let before = slots.snapshot();
        slots.replace(Frame { argc: 1, argv: 1 }).unwrap();
        slots.replace(before.frame).unwrap();
        assert!(matches!(
            slots.bind(Arc::new(command()), before),
            Err(Refusal::ArgumentsChanged)
        ));
        slots.0.lock().unwrap().revision = u64::MAX - 1;
        assert!(matches!(
            slots.bind(Arc::new(command()), slots.snapshot()),
            Err(Refusal::Exhausted)
        ));
        slots.replace(Frame::default()).unwrap();
        assert_eq!(slots.replace(Frame::default()), Err(Refusal::Exhausted));
    }
    #[test]
    fn concurrent_writer_does_not_get_overwritten_and_arguments_survive_conflict() {
        let slots = Arguments::new(Frame::default());
        let command = Arc::new(command());
        let weak = Arc::downgrade(&command);
        let binding = slots.bind(command, slots.snapshot()).unwrap();
        let published = slots.snapshot();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    slots
                        .replace(Frame {
                            argc: 0,
                            ..published.frame
                        })
                        .unwrap();
                })
                .join()
                .unwrap();
        });
        assert!(!binding.finish());
        assert_eq!(slots.snapshot().frame.argc, 0);
        assert_eq!(slots.snapshot().frame.argv, published.frame.argv);
        assert!(slots.retained());
        assert!(weak.upgrade().is_some());
        assert!(matches!(
            slots.bind(Arc::new(super::super::lab::command()), slots.snapshot()),
            Err(Refusal::ArgumentsBusy)
        ));
        assert_eq!(
            unsafe { slots.recover(published) },
            Err(Refusal::ArgumentsChanged)
        );
        assert_eq!(
            unsafe { slots.recover(slots.snapshot()) },
            Err(Refusal::ArgumentsStillPublished)
        );
        slots.replace(Frame::default()).unwrap();
        // Own writer joined, callback absent, no readers or copies of argv.
        unsafe { slots.recover(slots.snapshot()) }.unwrap();
        assert!(!slots.retained());
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn aba_during_binding_is_unknown_even_if_words_match_again() {
        let slots = Arguments::new(Frame::default());
        let binding = slots.bind(Arc::new(command()), slots.snapshot()).unwrap();
        let installed = slots.snapshot().frame;
        slots.replace(Frame::default()).unwrap();
        slots.replace(installed).unwrap();
        assert!(!binding.finish());
        assert!(slots.retained());
        slots.replace(Frame::default()).unwrap();
        unsafe { slots.recover(slots.snapshot()) }.unwrap();
    }
    #[test]
    fn dispatch_rechecks_thread_phase_world_deadline_and_action_command_identity() {
        let (mut gate, mut action) = ready();
        let d = Descriptor::new(original);
        let hook = d.install(original, replacement).unwrap();
        let slots = Arguments::new(Frame::default());
        for (c, w, now, error) in [
            (
                Context {
                    current_thread: 8,
                    ..context()
                },
                world(1),
                4,
                Error::WrongThread,
            ),
            (
                Context {
                    draw_depth: 1,
                    ..context()
                },
                world(1),
                4,
                Error::WrongPhase,
            ),
            (context(), world(2), 4, Error::InvalidWorld),
            (context(), world(1), 100, Error::InvalidTimeout),
            (context(), world(1), 3, Error::ClockReversed),
        ] {
            // Building the fixture snapshot may enter Windows synchronization
            // helpers. Measure only the replay call's LastError boundary.
            let arguments = slots.snapshot();
            #[cfg(windows)]
            unsafe {
                windows_sys::Win32::Foundation::SetLastError(0xabcd)
            };
            let result = replay(
                &mut gate,
                &action,
                &hook,
                &slots,
                Dispatch {
                    context: c,
                    world: w,
                    now,
                    arguments,
                },
            );
            #[cfg(windows)]
            let outgoing = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            assert_eq!(result.unwrap_err(), error);
            #[cfg(windows)]
            assert_eq!(outgoing, 0xabcd);
            assert!(gate.snapshot().in_flight);
            assert_eq!(slots.snapshot().frame, Frame::default());
        }
        // A public token copied onto an action from elsewhere cannot substitute argv.
        action.command = Some(Arc::new(
            OwnedCommand::new(&[b"map", b"different"]).unwrap(),
        ));
        assert_eq!(
            replay(
                &mut gate,
                &action,
                &hook,
                &slots,
                Dispatch {
                    context: context(),
                    world: world(1),
                    now: 4,
                    arguments: slots.snapshot()
                }
            )
            .unwrap_err(),
            Error::StaleAction
        );
        gate.acknowledge(action.token, Ack::NotInvoked, 4).unwrap();
        assert_eq!(gate.snapshot().phase, Phase::Recoverable);
    }
    #[test]
    fn dispatch_returns_original_value_does_not_recurse_and_cannot_replay_acknowledged_action() {
        let (mut gate, action) = ready();
        let d = Descriptor::new(original);
        let hook = d.install(original, replacement).unwrap();
        let slots = Arguments::new(Frame::default());
        let r = replay(
            &mut gate,
            &action,
            &hook,
            &slots,
            Dispatch {
                context: context(),
                world: world(1),
                now: 4,
                arguments: slots.snapshot(),
            },
        )
        .unwrap();
        assert!(r.invoked && r.arguments_restored && r.descriptor_owned);
        assert_eq!(r.returned, Some(11));
        assert_eq!(gate.snapshot().phase, Phase::AwaitWorld);
        assert_eq!(
            replay(
                &mut gate,
                &action,
                &hook,
                &slots,
                Dispatch {
                    context: context(),
                    world: world(1),
                    now: 4,
                    arguments: slots.snapshot()
                }
            )
            .unwrap_err(),
            Error::StaleAction
        );
        assert_eq!(d.call(), 22);
    }
}
