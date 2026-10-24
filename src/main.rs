mod api;
mod app;
mod session;
mod ui;

use std::{error::Error, io, time::Duration};

use app::{Effect, Model, Msg, update};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use gstreamer_play::{Play, PlaySignalAdapter, PlayVideoRenderer};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::{sync::mpsc, time};

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    gstreamer::init()?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = run(&mut terminal).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn run(terminal: &mut Tui) -> Result<(), Box<dyn Error>> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let player = Play::new(None::<PlayVideoRenderer>);
    let mut config = player.config();
    config.set_user_agent(
        "User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:100.0) Gecko/20100101 Firefox/100.0",
    );
    config.set_position_update_interval(250);
    config.set_seek_accurate(true);
    player.set_config(config)?;
    player.set_volume(0.7);

    let player_signals = PlaySignalAdapter::new(&player);
    let signal_sender = sender.clone();
    player_signals.connect_error(move |_, error, _| {
        let _ = signal_sender.send(Msg::PlaybackError(error.to_string()));
    });
    let signal_sender = sender.clone();
    player_signals.connect_end_of_stream(move |_| {
        let _ = signal_sender.send(Msg::PlaybackEnded);
    });

    let mut model = Model::default();
    dispatch(update(&mut model, Msg::Start), sender.clone(), &player);

    let mut tick = time::interval(Duration::from_millis(900));
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let main_context = glib::MainContext::default();
    while !model.should_quit {
        while main_context.pending() {
            main_context.iteration(false);
        }
        terminal.draw(|frame| ui::draw(frame, &model))?;

        if event::poll(Duration::ZERO)? {
            if let Event::Key(key_event) = event::read()? {
                dispatch(
                    update(&mut model, Msg::Key(key_event.code)),
                    sender.clone(),
                    &player,
                );
            }
        }

        tokio::select! {
            Some(msg) = receiver.recv() => {
                dispatch(update(&mut model, msg), sender.clone(), &player);
            }
            _ = tick.tick() => {
                dispatch(update(&mut model, Msg::Tick), sender.clone(), &player);
                if let Some(position) = player.position() {
                    let duration = player.duration().map(|time| time.seconds()).unwrap_or(0);
                    dispatch(
                        update(&mut model, Msg::PlaybackProgress {
                            position: position.seconds(),
                            duration,
                        }),
                        sender.clone(),
                        &player,
                    );
                }
            }
            _ = time::sleep(Duration::from_millis(30)) => {}
        }
    }
    player.stop();
    Ok(())
}

fn dispatch(effect: Option<Effect>, sender: mpsc::UnboundedSender<Msg>, player: &Play) {
    let Some(effect) = effect else {
        return;
    };
    if let Effect::StartPlayback { track, url } = effect {
        player.stop();
        player.set_uri(Some(&url));
        player.play();
        let _ = sender.send(Msg::PlaybackStarted(track));
        return;
    }
    if let Effect::SetPlayback(is_playing) = effect {
        if is_playing {
            player.play();
        } else {
            player.pause();
        }
        let _ = sender.send(Msg::PlaybackStateChanged(is_playing));
        return;
    }
    tokio::spawn(async move {
        let msg = match effect {
            Effect::CheckSession => Msg::SessionChecked(api::check_session().await),
            Effect::RequestQrKey => Msg::QrKeyReady(api::request_qr_key().await),
            Effect::PollQr(key) => Msg::QrPolled(api::poll_qr(&key).await),
            Effect::CompleteLogin(cookie) => Msg::LoginComplete(api::complete_login(cookie).await),
            Effect::LoadPlaylists(session) => {
                Msg::PlaylistsLoaded(api::load_playlists(session).await)
            }
            Effect::LoadSongs { id, title, session } => Msg::SongsLoaded {
                title,
                result: api::load_songs(id, session).await,
            },
            Effect::ResolveSongUrl { track, session } => {
                let result = api::song_url(track.id, session)
                    .await
                    .map(|url| (track, url));
                Msg::SongUrlLoaded(result)
            }
            Effect::StartPlayback { .. } => unreachable!(),
            Effect::SetPlayback(_) => unreachable!(),
        };
        let _ = sender.send(msg);
    });
}
