//! Asks the accessibility API whether the system-wide focused element can take
//! text, so a dictation is not pasted into the void (no field focused, a
//! button or list has focus) but parked in the clipboard instead.
//!
//! The probe is deliberately conservative: only a positive "nothing editable
//! has focus" answer stops the paste. Apps with sparse accessibility (custom
//! terminals, games, some cross-platform toolkits) often report a bare window
//! or group as focused even while a text cursor is active — those land in
//! `Unknown` and keep the old behaviour (paste as before).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    /// A text field, text area or another element whose value can be set.
    Editable,
    /// No element has focus, or the focused element is a known non-text
    /// control (button, list, table, menu, …).
    NotEditable,
    /// The accessibility API could not tell (no permission, timeout, app with
    /// sparse accessibility support). Callers paste as usual.
    Unknown,
}

/// Roles that always accept typed text.
const TEXT_ROLES: &[&str] = &["AXTextField", "AXTextArea", "AXComboBox"];

/// Roles that never accept typed text. Containers such as `AXWindow`,
/// `AXGroup` or `AXScrollArea` are intentionally missing: apps without proper
/// accessibility report those while a custom text view has the cursor.
const NON_TEXT_ROLES: &[&str] = &[
    "AXButton",
    "AXCell",
    "AXCheckBox",
    "AXColumn",
    "AXDisclosureTriangle",
    "AXImage",
    "AXLink",
    "AXList",
    "AXMenu",
    "AXMenuBar",
    "AXMenuBarItem",
    "AXMenuButton",
    "AXMenuItem",
    "AXOutline",
    "AXPopUpButton",
    "AXRadioButton",
    "AXRow",
    "AXSlider",
    "AXStaticText",
    "AXTabGroup",
    "AXTable",
    "AXToolbar",
    "AXWebArea",
];

fn classify(role: Option<&str>, value_settable: bool) -> FocusTarget {
    match role {
        Some(role) if TEXT_ROLES.contains(&role) => FocusTarget::Editable,
        _ if value_settable => FocusTarget::Editable,
        Some(role) if NON_TEXT_ROLES.contains(&role) => FocusTarget::NotEditable,
        _ => FocusTarget::Unknown,
    }
}

#[cfg(target_os = "macos")]
pub fn focused_text_target() -> FocusTarget {
    macos::focused_text_target()
}

#[cfg(not(target_os = "macos"))]
pub fn focused_text_target() -> FocusTarget {
    FocusTarget::Unknown
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    use core_foundation::{
        base::{CFRelease, CFType, CFTypeRef, TCFType},
        string::{CFString, CFStringRef},
    };

    use super::{FocusTarget, classify};

    type AXUIElementRef = *const c_void;
    type AXError = i32;

    const AX_ERROR_SUCCESS: AXError = 0;
    /// The attribute exists but has no value — for the system-wide element's
    /// focused-element attribute this means nothing has keyboard focus.
    const AX_ERROR_NO_VALUE: AXError = -25212;
    /// Upper bound for each accessibility round trip into the target app, so a
    /// hung app can't stall the dictation pipeline.
    const MESSAGING_TIMEOUT_SECS: f32 = 0.25;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        fn AXUIElementIsAttributeSettable(
            element: AXUIElementRef,
            attribute: CFStringRef,
            settable: *mut u8,
        ) -> AXError;
        fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    }

    /// Owns a +1 retained AXUIElementRef.
    struct Element(AXUIElementRef);

    impl Drop for Element {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    impl Element {
        /// Returns the attribute value with a +1 retain count.
        fn copy_raw(&self, name: &str) -> Result<CFTypeRef, AXError> {
            let attribute = CFString::new(name);
            let mut value: CFTypeRef = std::ptr::null();
            let err = unsafe {
                AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
            };
            if err != AX_ERROR_SUCCESS {
                return Err(err);
            }
            if value.is_null() {
                return Err(AX_ERROR_NO_VALUE);
            }
            Ok(value)
        }

        fn copy_attribute(&self, name: &str) -> Result<CFType, AXError> {
            self.copy_raw(name)
                .map(|value| unsafe { CFType::wrap_under_create_rule(value) })
        }

        fn is_settable(&self, name: &str) -> bool {
            let attribute = CFString::new(name);
            let mut settable: u8 = 0;
            let err = unsafe {
                AXUIElementIsAttributeSettable(
                    self.0,
                    attribute.as_concrete_TypeRef(),
                    &mut settable,
                )
            };
            err == AX_ERROR_SUCCESS && settable != 0
        }
    }

    pub fn focused_text_target() -> FocusTarget {
        let system = Element(unsafe { AXUIElementCreateSystemWide() });
        if system.0.is_null() {
            return FocusTarget::Unknown;
        }
        unsafe { AXUIElementSetMessagingTimeout(system.0, MESSAGING_TIMEOUT_SECS) };

        let focused = match system.copy_raw("AXFocusedUIElement") {
            Ok(value) => Element(value),
            Err(AX_ERROR_NO_VALUE) => {
                log::info!(target: "bridge", "focus probe: no focused element");
                return FocusTarget::NotEditable;
            }
            Err(err) => {
                log::info!(target: "bridge", "focus probe: focused element unavailable (AXError {err})");
                return FocusTarget::Unknown;
            }
        };
        unsafe { AXUIElementSetMessagingTimeout(focused.0, MESSAGING_TIMEOUT_SECS) };

        let role = focused
            .copy_attribute("AXRole")
            .ok()
            .and_then(|value| value.downcast::<CFString>())
            .map(|role| role.to_string());
        let value_settable = focused.is_settable("AXValue");
        let target = classify(role.as_deref(), value_settable);
        log::info!(
            target: "bridge",
            "focus probe: role {} (value settable: {value_settable}) -> {target:?}",
            role.as_deref().unwrap_or("<none>")
        );
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_roles_are_editable() {
        assert_eq!(classify(Some("AXTextField"), false), FocusTarget::Editable);
        assert_eq!(classify(Some("AXTextArea"), false), FocusTarget::Editable);
        assert_eq!(classify(Some("AXComboBox"), false), FocusTarget::Editable);
    }

    #[test]
    fn settable_value_wins_over_role() {
        assert_eq!(classify(Some("AXWebArea"), true), FocusTarget::Editable);
        assert_eq!(classify(None, true), FocusTarget::Editable);
    }

    #[test]
    fn known_controls_are_not_editable() {
        assert_eq!(classify(Some("AXButton"), false), FocusTarget::NotEditable);
        assert_eq!(classify(Some("AXOutline"), false), FocusTarget::NotEditable);
        assert_eq!(classify(Some("AXWebArea"), false), FocusTarget::NotEditable);
    }

    #[test]
    fn containers_and_missing_roles_stay_unknown() {
        assert_eq!(classify(Some("AXWindow"), false), FocusTarget::Unknown);
        assert_eq!(classify(Some("AXGroup"), false), FocusTarget::Unknown);
        assert_eq!(classify(None, false), FocusTarget::Unknown);
    }
}
