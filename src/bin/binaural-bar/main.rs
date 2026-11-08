//! Menu bar front end: a status item whose menu plays, pauses and picks settings.
//!
//! AppKit delivers menu actions on the main thread. Stream events arrive on the audio thread and
//! are posted to the main queue, so all state lives on the main thread behind one controller.

mod player;

use std::cell::{OnceCell, RefCell};
use std::process::ExitCode;

use binaural::audio::StreamEvent;
use binaural::logging::{Level, Logger, project_log_dir};
use binaural::settings::Settings;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
    NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSString, ns_string};

use crate::player::{
    BEAT_CHOICES, CARRIER_CHOICES, Player, VOLUME_CHOICES, beat_label, ears_label, hz_label,
    status_title,
};

thread_local! {
    /// The app's only controller. AppKit and the main dispatch queue both run on this thread.
    static CONTROLLER: OnceCell<Retained<Controller>> = const { OnceCell::new() };
}

/// Menu items that change as the state changes.
struct Items {
    status: Retained<NSStatusItem>,
    play: Retained<NSMenuItem>,
    ears: Retained<NSMenuItem>,
    output: Retained<NSMenuItem>,
    error: Retained<NSMenuItem>,
    beat: Retained<NSMenuItem>,
    carrier: Retained<NSMenuItem>,
    volume: Retained<NSMenuItem>,
    beats: Vec<Retained<NSMenuItem>>,
    carriers: Vec<Retained<NSMenuItem>>,
    volumes: Vec<Retained<NSMenuItem>>,
}

/// Controller state: the player, and the menu once it is built.
struct Ivars {
    player: RefCell<Player>,
    items: OnceCell<Items>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Controller does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BinauralController"]
    #[ivars = Ivars]
    struct Controller;

    impl Controller {
        #[unsafe(method(togglePlay:))]
        fn toggle_play(&self, _sender: &NSMenuItem) {
            self.run(|player| {
                if player.is_playing() { player.pause() } else { player.play(post_stream_event) }
            });
        }

        #[unsafe(method(selectBeat:))]
        fn select_beat(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            self.run(|player| {
                let hz = choice(&BEAT_CHOICES, tag)?;
                player.update(|settings| settings.beat_hz = hz)
            });
        }

        #[unsafe(method(selectCarrier:))]
        fn select_carrier(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            self.run(|player| {
                let hz = choice(&CARRIER_CHOICES, tag)?;
                player.update(|settings| settings.carrier_hz = hz)
            });
        }

        #[unsafe(method(selectVolume:))]
        fn select_volume(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            self.run(|player| {
                let pct = choice(&VOLUME_CHOICES, tag)?;
                player.update(|settings| settings.volume_pct = pct)
            });
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &NSMenuItem) {
            self.run(Player::quit);
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }

    unsafe impl NSObjectProtocol for Controller {}
);

impl Controller {
    /// Creates the controller around `player`.
    fn new(mtm: MainThreadMarker, player: Player) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            player: RefCell::new(player),
            items: OnceCell::new(),
        });
        // SAFETY: NSObject's `init` takes no arguments and returns the initialized object.
        unsafe { msg_send![super(this), init] }
    }

    /// Runs `action` on the player, keeps any error for the menu, and refreshes the menu.
    fn run(&self, action: impl FnOnce(&mut Player) -> Result<(), String>) {
        let mut player = self.ivars().player.borrow_mut();
        if let Err(error) = action(&mut player) {
            player.record_error(error);
        }
        drop(player);
        self.refresh();
    }

    /// Builds the status item and its menu.
    fn install_menu(&self, mtm: MainThreadMarker) {
        let menu = NSMenu::new(mtm);
        let play = self.item(mtm, "Play", Some(sel!(togglePlay:)), 0);
        let ears = self.item(mtm, "", None, 0);
        let output = self.item(mtm, "", None, 0);
        let error = self.item(mtm, "", None, 0);
        let beat_labels = BEAT_CHOICES.map(beat_label);
        let carrier_labels = CARRIER_CHOICES.map(hz_label);
        let volume_labels = VOLUME_CHOICES.map(|pct| format!("{pct}%"));
        let (beat, beats) = self.choice_menu(mtm, &beat_labels, sel!(selectBeat:));
        let (carrier, carriers) = self.choice_menu(mtm, &carrier_labels, sel!(selectCarrier:));
        let (volume, volumes) = self.choice_menu(mtm, &volume_labels, sel!(selectVolume:));
        let quit = self.item(mtm, "Quit Binaural", Some(sel!(quit:)), 0);
        quit.setKeyEquivalent(ns_string!("q"));

        for item in [&play, &ears, &output, &error] {
            menu.addItem(item);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        for item in [&beat, &carrier, &volume] {
            menu.addItem(item);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&quit);

        let status =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        status.setMenu(Some(&menu));
        let items = Items {
            status,
            play,
            ears,
            output,
            error,
            beat,
            carrier,
            volume,
            beats,
            carriers,
            volumes,
        };
        if self.ivars().items.set(items).is_err() {
            let error = "menu bar menu was built twice; this is a bug".to_string();
            self.ivars().player.borrow_mut().record_error(error);
        }
        self.refresh();
    }

    /// A menu item; with an action it targets this controller, without one it is greyed out.
    fn item(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
        tag: isize,
    ) -> Retained<NSMenuItem> {
        // SAFETY: every action selector passed here is a method defined on Controller, which
        // lives in CONTROLLER for the whole process and so outlives the menu.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                ns_string!(""),
            )
        };
        if action.is_some() {
            let target: &AnyObject = self;
            // SAFETY: the target implements `action`, as above.
            unsafe { item.setTarget(Some(target)) };
        }
        item.setTag(tag);
        item
    }

    /// A parent item with a submenu of `labels`, each tagged with its index and sending `action`.
    fn choice_menu(
        &self,
        mtm: MainThreadMarker,
        labels: &[String],
        action: Sel,
    ) -> (Retained<NSMenuItem>, Vec<Retained<NSMenuItem>>) {
        let submenu = NSMenu::new(mtm);
        let items: Vec<_> = labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let item = self.item(mtm, label, Some(action), index as isize);
                submenu.addItem(&item);
                item
            })
            .collect();
        let parent = self.item(mtm, "", None, 0);
        parent.setSubmenu(Some(&submenu));
        (parent, items)
    }

    /// Brings every title, check mark and the status text in line with the player.
    fn refresh(&self) {
        let Some(items) = self.ivars().items.get() else {
            return;
        };
        let player = self.ivars().player.borrow();
        let settings = player.settings();
        let playing = player.is_playing();

        if let Some(button) = items.status.button(self.mtm()) {
            let title = status_title(settings, playing, player.error().is_some());
            button.setTitle(&NSString::from_str(&title));
        }
        items
            .play
            .setTitle(&NSString::from_str(if playing { "Pause" } else { "Play" }));
        items
            .ears
            .setTitle(&NSString::from_str(&ears_label(settings)));
        let output = player
            .device()
            .map_or("Output: opens on Play".into(), |d| format!("Output: {d}"));
        items.output.setTitle(&NSString::from_str(&output));
        items
            .error
            .setTitle(&NSString::from_str(player.error().unwrap_or_default()));
        items.error.setHidden(player.error().is_none());

        let beat = format!("Beat: {}", beat_label(settings.beat_hz));
        items.beat.setTitle(&NSString::from_str(&beat));
        let carrier = format!("Carrier: {}", hz_label(settings.carrier_hz));
        items.carrier.setTitle(&NSString::from_str(&carrier));
        items.volume.setTitle(&NSString::from_str(&format!(
            "Volume: {}%",
            settings.volume_pct
        )));
        check(
            &items.beats,
            BEAT_CHOICES.iter().position(|hz| *hz == settings.beat_hz),
        );
        check(
            &items.carriers,
            CARRIER_CHOICES
                .iter()
                .position(|hz| *hz == settings.carrier_hz),
        );
        check(
            &items.volumes,
            VOLUME_CHOICES
                .iter()
                .position(|pct| *pct == settings.volume_pct),
        );
    }
}

/// Puts a check mark on the item at `selected` and clears the rest.
fn check(items: &[Retained<NSMenuItem>], selected: Option<usize>) {
    for (index, item) in items.iter().enumerate() {
        let state = if Some(index) == selected {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        };
        item.setState(state);
    }
}

/// The choice a menu item's tag points at.
fn choice<T: Copy>(choices: &[T], tag: isize) -> Result<T, String> {
    usize::try_from(tag)
        .ok()
        .and_then(|index| choices.get(index).copied())
        .ok_or_else(|| {
            format!(
                "menu item tag {tag} is outside {} choices; this is a bug",
                choices.len()
            )
        })
}

/// Passes a stream event from the audio thread to the controller on the main thread.
fn post_stream_event(stream_id: u64, event: StreamEvent) {
    DispatchQueue::main().exec_async(move || {
        CONTROLLER.with(|cell| {
            if let Some(controller) = cell.get() {
                controller.run(|player| player.stream_event(stream_id, event));
            }
        });
    });
}

fn main() -> ExitCode {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("error: starting menu bar app off the main thread; run the binary directly");
        return ExitCode::FAILURE;
    };
    let mut logger = match Logger::create(&project_log_dir()) {
        Ok(logger) => logger,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = logger.log(Level::Info, "menu bar app started") {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }

    let app = NSApplication::sharedApplication(mtm);
    // Accessory: a menu bar item only, with no Dock icon or app menu.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let player = Player::new(Settings::default(), logger);
    CONTROLLER.with(|cell| {
        cell.get_or_init(|| Controller::new(mtm, player))
            .install_menu(mtm)
    });
    app.run();
    ExitCode::SUCCESS
}
