use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, define_class, extern_methods, msg_send,
    rc::Retained, runtime::Bool,
};
use objc2_app_kit::{NSApp, NSApplication, NSEvent};
use objc2_foundation::NSObjectProtocol;
use std::cell::Cell;

#[derive(Default)]
pub struct ProbeApplicationIvars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    /// CEF requires the process-wide NSApplication to implement its event protocols.
    /// This remains the AppKit lifecycle owner; CEF only owns its browser child NSView.
    #[unsafe(super(NSApplication))]
    #[ivars = ProbeApplicationIvars]
    pub struct ProbeApplication;

    impl ProbeApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let was_sending_event = self.is_handling_send_event();
            if !was_sending_event {
                self.set_handling_send_event(true);
            }

            let _: () = unsafe { msg_send![super(self), sendEvent:event] };

            if !was_sending_event {
                self.set_handling_send_event(false);
            }
        }
    }

    unsafe impl CrAppControlProtocol for ProbeApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_cef_handling_send_event(&self, handling_send_event: Bool) {
            self.ivars().handling_send_event.set(handling_send_event);
        }
    }

    unsafe impl CrAppProtocol for ProbeApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn cef_is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for ProbeApplication {}
);

impl ProbeApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared_application() -> Retained<Self>;

        #[unsafe(method(setHandlingSendEvent:))]
        fn set_handling_send_event(&self, handling_send_event: bool);

        #[unsafe(method(isHandlingSendEvent))]
        fn is_handling_send_event(&self) -> bool;
    }
}

pub fn initialize() -> Result<(), &'static str> {
    let _application = ProbeApplication::shared_application();
    let main_thread =
        MainThreadMarker::new().ok_or("AppKit initialization is not on main thread")?;
    if !NSApp(main_thread).isKindOfClass(ProbeApplication::class()) {
        return Err("NSApplication existed before CEF protocol subclass initialization");
    }
    Ok(())
}
