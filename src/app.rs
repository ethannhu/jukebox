use crossterm::event::KeyCode;

pub struct Model {
    pub status: String,
    pub qr_url: Option<String>,
    pub playlists: Vec<PlaylistEntry>,
    pub selected: usize,
    saved_playlist_selection: usize,
    pub page: Page,
    pub show_controller: bool,
    pub queue_title: Option<String>,
    pub queue: Vec<Track>,
    pub queue_selected: usize,
    pub current_track: Option<Track>,
    pub playback_position: u64,
    pub playback_duration: u64,
    pub is_playing: bool,
    pub session: Option<Session>,
    qr_key: Option<String>,
    qr_poll_in_flight: bool,
    pub should_quit: bool,
}

#[derive(Default)]
pub enum Page {
    #[default]
    Playlists,
    PlaylistDetail {
        title: String,
        songs: Vec<Track>,
    },
}

#[derive(Clone)]
pub struct Session {
    pub cookie: String,
    pub uid: u64,
}

pub struct PlaylistEntry {
    pub id: u64,
    pub name: String,
    pub label: String,
}

#[derive(Clone)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub artists: String,
}

impl Track {
    pub fn label(&self) -> String {
        if self.artists.is_empty() {
            self.name.clone()
        } else {
            format!("{} — {}", self.name, self.artists)
        }
    }
}

pub enum QrPoll {
    Waiting,
    Scanned,
    Expired,
    Authorized(String),
}

pub enum Msg {
    Start,
    Key(KeyCode),
    Tick,
    SessionChecked(Result<Option<Session>, String>),
    QrKeyReady(Result<String, String>),
    QrPolled(Result<QrPoll, String>),
    LoginComplete(Result<Session, String>),
    PlaylistsLoaded(Result<Vec<PlaylistEntry>, String>),
    SongsLoaded {
        title: String,
        result: Result<Vec<Track>, String>,
    },
    SongUrlLoaded(Result<(Track, String), String>),
    PlaybackStarted(Track),
    PlaybackStateChanged(bool),
    PlaybackProgress {
        position: u64,
        duration: u64,
    },
    PlaybackError(String),
    PlaybackEnded,
}

pub enum Effect {
    CheckSession,
    RequestQrKey,
    PollQr(String),
    CompleteLogin(String),
    LoadPlaylists(Session),
    LoadSongs {
        id: u64,
        title: String,
        session: Session,
    },
    ResolveSongUrl {
        track: Track,
        session: Session,
    },
    SetPlayback(bool),
    StartPlayback {
        track: Track,
        url: String,
    },
}

impl Default for Model {
    fn default() -> Self {
        Self {
            status: "正在检查已保存的登录状态…".into(),
            qr_url: None,
            playlists: Vec::new(),
            selected: 0,
            saved_playlist_selection: 0,
            page: Page::Playlists,
            show_controller: false,
            queue_title: None,
            queue: Vec::new(),
            queue_selected: 0,
            current_track: None,
            playback_position: 0,
            playback_duration: 0,
            is_playing: false,
            session: None,
            qr_key: None,
            qr_poll_in_flight: false,
            should_quit: false,
        }
    }
}

pub fn update(model: &mut Model, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Start => Some(Effect::CheckSession),
        Msg::Key(KeyCode::Char('q')) => {
            model.should_quit = true;
            None
        }
        Msg::Key(KeyCode::Char('p')) => {
            model.show_controller = !model.show_controller;
            model.status = if model.show_controller {
                "播放控制器 · Enter 播放 · 空格暂停/继续 · p 返回歌单页".into()
            } else {
                model
                    .session
                    .as_ref()
                    .map(|session| playlist_status(session.uid))
                    .unwrap_or_default()
            };
            None
        }
        Msg::Key(KeyCode::Char(' ')) if model.current_track.is_some() => {
            Some(Effect::SetPlayback(!model.is_playing))
        }
        Msg::Key(KeyCode::Char('j') | KeyCode::Left) if model.current_track.is_some() => {
            play_relative_track(model, false)
        }
        Msg::Key(KeyCode::Char('k') | KeyCode::Right) if model.current_track.is_some() => {
            play_relative_track(model, true)
        }
        Msg::Key(KeyCode::Esc) => match &model.page {
            Page::PlaylistDetail { .. } => {
                if model.show_controller {
                    model.show_controller = false;
                }
                model.page = Page::Playlists;
                model.selected = model.saved_playlist_selection;
                model.status = model
                    .session
                    .as_ref()
                    .map(|session| playlist_status(session.uid))
                    .unwrap_or_default();
                None
            }
            Page::Playlists => {
                if model.show_controller {
                    model.show_controller = false;
                    model.status = model
                        .session
                        .as_ref()
                        .map(|session| playlist_status(session.uid))
                        .unwrap_or_default();
                    return None;
                }
                model.should_quit = true;
                None
            }
        },
        Msg::Key(KeyCode::Up) => {
            if model.show_controller {
                model.queue_selected = model.queue_selected.saturating_sub(1);
            } else {
                model.selected = model.selected.saturating_sub(1);
            }
            None
        }
        Msg::Key(KeyCode::Down) => {
            if model.show_controller {
                if !model.queue.is_empty() {
                    model.queue_selected = (model.queue_selected + 1).min(model.queue.len() - 1);
                }
            } else {
                let len = match &model.page {
                    Page::Playlists => model.playlists.len(),
                    Page::PlaylistDetail { songs, .. } => songs.len(),
                };
                if len > 0 {
                    model.selected = (model.selected + 1).min(len - 1);
                }
            }
            None
        }
        Msg::Key(KeyCode::Enter) => {
            if model.show_controller {
                let Some(track) = model.queue.get(model.queue_selected).cloned() else {
                    model.status = "播放列表为空".into();
                    return None;
                };
                let Some(session) = model.session.clone() else {
                    model.status = "请先登录".into();
                    return None;
                };
                model.status = format!("正在获取播放地址：{}", track.name);
                return Some(Effect::ResolveSongUrl { track, session });
            }
            if !matches!(&model.page, Page::Playlists) {
                return None;
            }
            let Some(playlist) = model.playlists.get(model.selected) else {
                return None;
            };
            let Some(session) = model.session.clone() else {
                return None;
            };
            model.saved_playlist_selection = model.selected;
            model.status = format!("正在加载 {} …", playlist.name);
            Some(Effect::LoadSongs {
                id: playlist.id,
                title: playlist.name.clone(),
                session,
            })
        }
        Msg::Tick => {
            if let Some(key) = model.qr_key.clone().filter(|_| !model.qr_poll_in_flight) {
                model.qr_poll_in_flight = true;
                Some(Effect::PollQr(key))
            } else {
                None
            }
        }
        Msg::PlaybackProgress { position, duration } => {
            model.playback_position = position;
            model.playback_duration = duration;
            None
        }
        Msg::SessionChecked(Ok(Some(session))) => {
            model.status = "已恢复登录状态，正在加载歌单…".into();
            model.session = Some(session.clone());
            Some(Effect::LoadPlaylists(session))
        }
        Msg::SessionChecked(Ok(None)) => {
            model.status = "需要登录，正在创建二维码…".into();
            Some(Effect::RequestQrKey)
        }
        Msg::SessionChecked(Err(error)) => {
            model.status = format!("检查登录状态失败：{error} · 按 q 退出");
            None
        }
        Msg::QrKeyReady(Ok(key)) => {
            model.qr_url = Some(format!("https://music.163.com/login?codekey={key}"));
            model.qr_key = Some(key);
            model.status = "请用网易云音乐 App 扫码…".into();
            None
        }
        Msg::QrKeyReady(Err(error)) => {
            model.status = format!("创建二维码失败：{error} · 按 q 退出");
            None
        }
        Msg::QrPolled(result) => {
            model.qr_poll_in_flight = false;
            match result {
                Ok(QrPoll::Waiting) => model.status = "等待扫码…".into(),
                Ok(QrPoll::Scanned) => model.status = "已扫码，请在手机上确认登录…".into(),
                Ok(QrPoll::Expired) => {
                    model.status = "二维码已过期，请重启程序重新登录。".into();
                    model.qr_key = None;
                    model.qr_url = None;
                }
                Ok(QrPoll::Authorized(cookie)) => {
                    model.status = "登录成功，正在确认用户信息…".into();
                    model.qr_key = None;
                    model.qr_url = None;
                    return Some(Effect::CompleteLogin(cookie));
                }
                Err(error) => model.status = format!("检查扫码状态失败：{error} · 继续等待…"),
            }
            None
        }
        Msg::LoginComplete(Ok(session)) => {
            model.status = "登录成功，正在加载歌单…".into();
            model.session = Some(session.clone());
            Some(Effect::LoadPlaylists(session))
        }
        Msg::LoginComplete(Err(error)) => {
            model.status = format!("登录确认失败：{error} · 按 q 退出");
            None
        }
        Msg::PlaylistsLoaded(Ok(playlists)) => {
            model.playlists = playlists;
            model.selected = 0;
            model.status = model
                .session
                .as_ref()
                .map(|session| playlist_status(session.uid))
                .unwrap_or_default();
            None
        }
        Msg::PlaylistsLoaded(Err(error)) => {
            model.status = format!("获取歌单失败：{error} · 按 q 退出");
            None
        }
        Msg::SongsLoaded { title, result } => match result {
            Ok(songs) => {
                model.selected = 0;
                model.queue_title = Some(title.clone());
                model.queue = songs.clone();
                model.queue_selected = 0;
                model.status = format!("{} 首歌曲 · Esc 返回歌单列表", songs.len());
                model.page = Page::PlaylistDetail { title, songs };
                None
            }
            Err(error) => {
                model.status = format!("加载歌单失败：{error}");
                None
            }
        },
        Msg::SongUrlLoaded(Ok((track, url))) => {
            model.status = format!("正在播放：{}", track.name);
            Some(Effect::StartPlayback { track, url })
        }
        Msg::SongUrlLoaded(Err(error)) => {
            model.status = format!("获取播放地址失败：{error}");
            None
        }
        Msg::PlaybackStarted(track) => {
            model.current_track = Some(track);
            model.playback_position = 0;
            model.playback_duration = 0;
            model.is_playing = true;
            model.status = "正在播放".into();
            None
        }
        Msg::PlaybackStateChanged(is_playing) => {
            model.is_playing = is_playing;
            model.status = if is_playing {
                "正在播放".into()
            } else {
                "已暂停".into()
            };
            None
        }
        Msg::PlaybackError(error) => {
            model.is_playing = false;
            model.status = format!("播放失败：{error}");
            None
        }
        Msg::PlaybackEnded => {
            model.is_playing = false;
            model.status = "播放结束".into();
            None
        }
        Msg::Key(_) => None,
    }
}

fn play_relative_track(model: &mut Model, next: bool) -> Option<Effect> {
    if model.queue.is_empty() {
        return None;
    }
    let Some(session) = model.session.clone() else {
        return None;
    };
    let current_index = model
        .current_track
        .as_ref()
        .and_then(|current| model.queue.iter().position(|track| track.id == current.id))
        .unwrap_or(model.queue_selected);
    let len = model.queue.len();
    model.queue_selected = if next {
        (current_index + 1) % len
    } else {
        (current_index + len - 1) % len
    };
    let track = model.queue[model.queue_selected].clone();
    model.status = format!("正在切换到：{}", track.name);
    Some(Effect::ResolveSongUrl { track, session })
}

fn playlist_status(uid: u64) -> String {
    format!("用户 ID: {uid} · ↑/↓ 移动 · Enter 打开 · p 播放控制器 · q 退出")
}
