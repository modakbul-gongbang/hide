use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, define_class, extern_methods, msg_send,
    rc::Retained, runtime::Bool,
};
use objc2_app_kit::{NSApp, NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSObjectProtocol;
use std::cell::Cell;

#[derive(Default)]
pub struct IntegratedApplicationIvars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    /// CEF's process protocol is implemented by the same AppKit lifecycle owner.
    #[unsafe(super(NSApplication))]
    #[ivars = IntegratedApplicationIvars]
    pub struct IntegratedApplication;

    impl IntegratedApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            if event.r#type() == NSEventType::KeyDown
                && crate::app::is_zoom_shortcut(event.keyCode(), event.modifierFlags())
            {
                crate::app::route_zoom_shortcut();
                return;
            }
            if event.r#type() == NSEventType::KeyDown
                && crate::app::is_quit_shortcut(event.keyCode(), event.modifierFlags())
            {
                crate::app::request_orderly_quit();
                return;
            }

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

    unsafe impl CrAppControlProtocol for IntegratedApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_cef_handling_send_event(&self, handling_send_event: Bool) {
            self.ivars().handling_send_event.set(handling_send_event);
        }
    }

    unsafe impl CrAppProtocol for IntegratedApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn cef_is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for IntegratedApplication {}
);

impl IntegratedApplication {
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
    let _application = IntegratedApplication::shared_application();
    let mtm = MainThreadMarker::new().ok_or("CEF AppKit bootstrap is not on the main thread")?;
    if !NSApp(mtm).isKindOfClass(IntegratedApplication::class()) {
        return Err("NSApplication existed before the CEF-compatible subclass");
    }
    Ok(())
}

#[allow(dead_code)]
fn _modifier_type_anchor(_: NSEventModifierFlags) {}
