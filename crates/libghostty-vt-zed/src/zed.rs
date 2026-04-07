#![warn(clippy::pedantic)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(clippy::allow_attributes)]
#![warn(clippy::allow_attributes_without_reason)]

//! Thin Zed-specific adapter helpers built on top of `libghostty-vt`.

use std::{cell::RefCell, rc::Rc};

use libghostty_vt::{
    RenderState, Terminal,
    error::Result,
    style::RgbColor,
    terminal::{
        ColorScheme, ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType,
        PrimaryDeviceAttributes, SecondaryDeviceAttributes, SizeReportSize,
        TertiaryDeviceAttributes,
    },
};

/// Runtime effects collected from terminal callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEffect {
    /// Bell sequence was received.
    Bell,
    /// Terminal title changed, including being cleared.
    TitleChanged(Option<String>),
}

/// Options for installing Zed callback behavior on a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallOptions {
    /// XTVERSION response string to report.
    pub xtversion: &'static str,
}

#[derive(Debug, Default)]
struct CallbackState {
    pending_pty_writes: Vec<Vec<u8>>,
    pending_effects: Vec<RuntimeEffect>,
    columns: u16,
    rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
}

/// Shared callback queue and size state for a terminal instance.
#[derive(Debug, Clone)]
pub struct TerminalCallbacks {
    state: Rc<RefCell<CallbackState>>,
}

impl TerminalCallbacks {
    /// Create a callback queue for a terminal of the given size.
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            state: Rc::new(RefCell::new(CallbackState {
                columns,
                rows,
                cell_width_px: 1,
                cell_height_px: 1,
                ..CallbackState::default()
            })),
        }
    }

    /// Install Zed callback behavior on a terminal.
    ///
    /// Call this after the terminal has reached its final storage location.
    /// The underlying callback registration keeps terminal-owned callback
    /// vtables alive by address, so moving the terminal afterward can
    /// invalidate the callback userdata.
    pub fn install(
        &self,
        terminal: &mut Terminal<'static, 'static>,
        options: InstallOptions,
    ) -> Result<()> {
        {
            let callbacks = self.clone();
            terminal.on_pty_write(move |_terminal, data| callbacks.push_pty_write(data))?;
        }
        {
            let callbacks = self.clone();
            terminal.on_bell(move |_terminal| callbacks.push_effect(RuntimeEffect::Bell))?;
        }
        terminal.on_enquiry(|_terminal| None)?;
        terminal.on_xtversion(move |_terminal| Some(options.xtversion))?;
        {
            let callbacks = self.clone();
            terminal.on_title_changed(move |terminal| {
                let title = terminal
                    .title()
                    .ok()
                    .and_then(|title| (!title.is_empty()).then(|| title.to_owned()));
                callbacks.push_effect(RuntimeEffect::TitleChanged(title));
            })?;
        }
        terminal.on_color_scheme(|terminal| color_scheme_for_terminal(terminal))?;
        {
            let callbacks = self.clone();
            terminal.on_size(move |_terminal| {
                let state = callbacks.state.borrow();
                Some(SizeReportSize {
                    rows: state.rows,
                    columns: state.columns,
                    cell_width: state.cell_width_px,
                    cell_height: state.cell_height_px,
                })
            })?;
        }
        terminal.on_device_attributes(|_terminal| Some(default_device_attributes()))?;
        Ok(())
    }

    /// Update the terminal size reported by the size callback.
    pub fn update_size(&self, columns: u16, rows: u16, cell_width_px: u32, cell_height_px: u32) {
        let mut state = self.state.borrow_mut();
        state.columns = columns;
        state.rows = rows;
        state.cell_width_px = cell_width_px.max(1);
        state.cell_height_px = cell_height_px.max(1);
    }

    /// Drain queued PTY writes.
    pub fn drain_pty_writes(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.state.borrow_mut().pending_pty_writes)
    }

    /// Drain queued runtime effects.
    pub fn drain_effects(&self) -> Vec<RuntimeEffect> {
        std::mem::take(&mut self.state.borrow_mut().pending_effects)
    }

    fn push_pty_write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        self.state
            .borrow_mut()
            .pending_pty_writes
            .push(bytes.to_vec());
    }

    fn push_effect(&self, effect: RuntimeEffect) {
        self.state.borrow_mut().pending_effects.push(effect);
    }
}

fn color_scheme_for_terminal(terminal: &Terminal<'_, '_>) -> Option<ColorScheme> {
    let mut render_state = RenderState::new().ok()?;
    let snapshot = render_state.update(terminal).ok()?;
    let background = snapshot.colors().ok()?.background;
    Some(if rgb_is_light(background) {
        ColorScheme::Light
    } else {
        ColorScheme::Dark
    })
}

fn rgb_is_light(color: RgbColor) -> bool {
    let luminance =
        (0.299 * f32::from(color.r)) + (0.587 * f32::from(color.g)) + (0.114 * f32::from(color.b));
    luminance >= 128.0
}

fn default_device_attributes() -> DeviceAttributes {
    DeviceAttributes {
        primary: PrimaryDeviceAttributes::new(
            ConformanceLevel(62),
            [
                DeviceAttributeFeature(1),
                DeviceAttributeFeature(6),
                DeviceAttributeFeature(22),
            ],
        ),
        secondary: SecondaryDeviceAttributes {
            device_type: DeviceType(1),
            firmware_version: 1,
            rom_cartridge: 0,
        },
        tertiary: TertiaryDeviceAttributes { unit_id: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallOptions, RuntimeEffect, TerminalCallbacks};
    use libghostty_vt::{Terminal, TerminalOptions};

    fn install_callbacks() -> (Box<Terminal<'static, 'static>>, TerminalCallbacks) {
        let mut terminal = Box::new(
            Terminal::new(TerminalOptions {
                cols: 20,
                rows: 4,
                max_scrollback: 128,
            })
            .expect("create terminal"),
        );
        let callbacks = TerminalCallbacks::new(20, 4);
        callbacks
            .install(&mut terminal, InstallOptions { xtversion: "Zed" })
            .expect("install callbacks");
        (terminal, callbacks)
    }

    #[test]
    fn callbacks_queue_runtime_effects() {
        let (mut terminal, callbacks) = install_callbacks();

        terminal.vt_write(b"\x07");
        assert_eq!(callbacks.drain_effects(), vec![RuntimeEffect::Bell]);
        assert!(callbacks.drain_effects().is_empty());

        terminal.vt_write(b"\x1b]2;zed title\x07");
        assert_eq!(
            callbacks.drain_effects(),
            vec![RuntimeEffect::TitleChanged(Some("zed title".to_owned()))]
        );

        terminal.vt_write(b"\x1b]2;\x07");
        assert_eq!(
            callbacks.drain_effects(),
            vec![RuntimeEffect::TitleChanged(None)]
        );
    }

    #[test]
    fn callbacks_queue_pty_replies() {
        let (mut terminal, callbacks) = install_callbacks();

        terminal.vt_write(b"\x1b[>q");
        let replies = callbacks.drain_pty_writes();
        assert_eq!(replies.len(), 1, "expected one XTVERSION reply");
        let reply = String::from_utf8(replies.into_iter().next().expect("reply"))
            .expect("XTVERSION reply should be valid UTF-8");
        assert!(
            reply.contains("Zed"),
            "expected XTVERSION reply to contain Zed, got {reply:?}"
        );

        terminal.vt_write(b"\x05");
        assert!(
            callbacks.drain_pty_writes().is_empty(),
            "expected ENQ callback to stay silent"
        );
    }
}
