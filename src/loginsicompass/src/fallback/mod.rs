//! The software-rendered greeter: a password box and nothing else.
//!
//! This is what `--render-backend shm` runs, and it exists for one situation:
//! the Vulkan path could not start. A login screen that fails to come up
//! leaves no graphical way into the machine, so there is a second renderer
//! that needs no GPU stack at all — it draws with `tiny-skia` into a shared
//! memory buffer and asks the compositor for nothing but `wl_shm`.
//!
//! It is deliberately worse than the real greeter, and the gap is the whole
//! argument for the Vulkan one:
//!
//! * **No text.** No font is linked, so greetd's prompt is not shown, the
//!   username is not shown, and a failure message is not shown. A wrong
//!   password looks exactly like a right one except that the dots clear.
//! * **No accessibility.** No AccessKit, no AT-SPI. Mute to a screen reader.
//! * **No user or session picker.** One username and one command, both from
//!   the command line.
//!
//! So it is a way to get in and fix things, not a greeter anybody should meet
//! twice. Everything here predates the split and is unchanged by it.

pub mod color;
pub mod entry;
pub mod renderer;
pub mod state;
