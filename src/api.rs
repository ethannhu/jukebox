use ncm_api_rs::{CryptoType, Query, RequestOption, create_client};
use serde_json::{Value, json};

use crate::{
    app::{PlaylistEntry, QrPoll, Session, Track},
    session,
};

pub async fn check_session() -> Result<Option<Session>, String> {
    let Some(cookie) = session::load_cookie().map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let client = create_client(Some(cookie.clone()));
    match client.login_status(&Query::new()).await {
        Ok(response) => match extract_uid(&response.body) {
            Some(uid) => Ok(Some(Session { cookie, uid })),
            None => {
                session::clear_cookie().map_err(|error| error.to_string())?;
                Ok(None)
            }
        },
        Err(ncm_api_rs::NcmError::AuthRequired(_)) => {
            session::clear_cookie().map_err(|error| error.to_string())?;
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

pub async fn request_qr_key() -> Result<String, String> {
    let client = create_client(None);
    let response = client
        .request(
            "/api/login/qrcode/unikey",
            json!({"type": 1}),
            RequestOption {
                crypto: CryptoType::Weapi,
                ..RequestOption::default()
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    extract_qr_key(&response.body)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "二维码 key 响应中找不到 unikey（status={}，body={}）",
                response.status, response.body
            )
        })
}

pub async fn poll_qr(key: &str) -> Result<QrPoll, String> {
    let client = create_client(None);
    let response = client
        .request(
            "/api/login/qrcode/client/login",
            json!({"type": 1, "key": key}),
            RequestOption {
                crypto: CryptoType::Weapi,
                ..RequestOption::default()
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    match response.body["code"].as_i64().unwrap_or_default() {
        800 => Ok(QrPoll::Expired),
        801 => Ok(QrPoll::Waiting),
        802 => Ok(QrPoll::Scanned),
        803 => {
            let cookie = normalize_cookies(&response.cookie);
            if cookie.is_empty() {
                Err("扫码成功，但接口没有返回 Cookie".into())
            } else {
                Ok(QrPoll::Authorized(cookie))
            }
        }
        code => Err(format!("二维码状态异常（code={code}）：{}", response.body)),
    }
}

pub async fn complete_login(cookie: String) -> Result<Session, String> {
    let client = create_client(Some(cookie.clone()));
    let response = client
        .login_status(&Query::new())
        .await
        .map_err(|error| error.to_string())?;
    let uid = extract_uid(&response.body).ok_or("登录成功，但无法读取用户 ID")?;
    session::save_cookie(&cookie).map_err(|error| error.to_string())?;
    Ok(Session { cookie, uid })
}

pub async fn load_playlists(session: Session) -> Result<Vec<PlaylistEntry>, String> {
    let client = create_client(Some(session.cookie.clone()));
    let uid = session.uid.to_string();
    let response = client
        .user_playlist(
            &Query::new()
                .param("uid", &uid)
                .param("limit", "100")
                .param("offset", "0"),
        )
        .await
        .map_err(|error| error.to_string())?;
    let playlists = response.body["playlist"]
        .as_array()
        .ok_or_else(|| format!("歌单接口没有返回 playlist 数组：{}", response.body))?;
    Ok(playlists
        .iter()
        .map(|playlist| PlaylistEntry {
            id: playlist["id"].as_u64().unwrap_or_default(),
            name: playlist["name"]
                .as_str()
                .unwrap_or("(未命名歌单)")
                .to_string(),
            label: playlist_label(playlist, session.uid),
        })
        .collect())
}

pub async fn load_songs(id: u64, session: Session) -> Result<Vec<Track>, String> {
    let client = create_client(Some(session.cookie));
    let id = id.to_string();
    let response = client
        .playlist_track_all(
            &Query::new()
                .param("id", &id)
                .param("limit", "1000")
                .param("offset", "0"),
        )
        .await
        .map_err(|error| error.to_string())?;
    let songs = response.body["songs"]
        .as_array()
        .ok_or_else(|| format!("歌单歌曲响应没有 songs 数组：{}", response.body))?;
    Ok(songs.iter().map(song_from_json).collect())
}

pub async fn song_url(id: u64, session: Session) -> Result<String, String> {
    let client = create_client(Some(session.cookie));
    let id = id.to_string();
    let response = client
        .song_url_v1(&Query::new().param("id", &id).param("level", "standard"))
        .await
        .map_err(|error| error.to_string())?;
    response.body["data"][0]["url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("歌曲没有可用播放地址：{}", response.body))
}

fn extract_uid(body: &Value) -> Option<u64> {
    body["data"]["profile"]["userId"]
        .as_u64()
        .or_else(|| body["profile"]["userId"].as_u64())
        .or_else(|| body["data"]["account"]["id"].as_u64())
}

fn extract_qr_key(body: &Value) -> Option<&str> {
    body.get("unikey")
        .and_then(Value::as_str)
        .or_else(|| body.get("data")?.get("unikey")?.as_str())
}

fn normalize_cookies(cookies: &[String]) -> String {
    cookies
        .iter()
        .filter_map(|cookie| cookie.split(';').next())
        .map(str::trim)
        .filter(|cookie| cookie.contains('='))
        .collect::<Vec<_>>()
        .join("; ")
}

fn playlist_label(playlist: &Value, uid: u64) -> String {
    let name = playlist["name"].as_str().unwrap_or("(未命名歌单)");
    let creator_id = playlist["userId"].as_u64().unwrap_or_default();
    let kind = if creator_id == uid {
        "创建"
    } else {
        "收藏"
    };
    let count = playlist["trackCount"].as_u64().unwrap_or_default();
    format!("[{kind}] {name}  ({count} 首)")
}

fn song_from_json(song: &Value) -> Track {
    let name = song["name"].as_str().unwrap_or("(未知歌曲)");
    let artists = song["ar"]
        .as_array()
        .or_else(|| song["artists"].as_array())
        .map(|artists| {
            artists
                .iter()
                .filter_map(|artist| artist["name"].as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_default();
    Track {
        id: song["id"].as_u64().unwrap_or_default(),
        name: name.to_string(),
        artists,
    }
}

#[cfg(test)]
mod tests {
    use super::extract_qr_key;
    use serde_json::json;

    #[test]
    fn reads_qr_key_from_supported_response_shapes() {
        assert_eq!(
            extract_qr_key(&json!({"code": 200, "unikey": "top-level"})),
            Some("top-level")
        );
        assert_eq!(
            extract_qr_key(&json!({"code": 200, "data": {"unikey": "nested"}})),
            Some("nested")
        );
        assert_eq!(extract_qr_key(&json!({"code": 200})), None);
    }
}
