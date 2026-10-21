//! `tansu serve` の HTTP サーバー。Web 画面 (ビルド時に埋め込んだ `web/dist`) と HTTP API を配信する。
//! API の処理は `tansu-core` の関数を呼ぶだけにする (`documents/PROJECT.md`「構成」)。すべてのリクエストの前に `Host` の検査を、
//! API のリクエストの前にトークンの検査を、書き込みのリクエストの前に CSRF 対策を置く (`documents/PROJECT.md`「制約」)。

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tansu_core::config::Settings;
use tansu_core::db::{self, rows, schema};
use tansu_core::model::Database;
use tansu_core::qa::input::{self, InputSession};
use tansu_core::qa::record::{self, ItemSelector, RecordRequest};
use tansu_core::{Error, api_token, export, named_json, qa, usage};

/// `--host` の既定。ログインが無いため、LAN に開くのは `--host 0.0.0.0` を明示した時だけにする (`documents/PROJECT.md`「制約」)。
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// `--port` と環境変数 `TANSU_PORT` の既定。開発マシンで同時に動く他の dev サーバーの既定 (3000 / 5173 / 8080 /
/// agent-timeline の 7878) と重ならない番号 (`documents/PROJECT.md`「CLI (汎用)」の serve の行。`Makefile` の `TANSU_PORT` と同じ値)。
pub const DEFAULT_PORT: u16 = 7979;

/// `Host` の許可リストに常に入れるホスト名 (このマシンの中からの接続)。
const LOOPBACK_HOSTS: [&str; 2] = ["127.0.0.1", "localhost"];

/// 画面の読み込みのたびに利用記録 (`usage.jsonl`) に書く `event`。`documents/DIRECTION.md`「判定基準」の
/// 「Web 画面を開いた日数」の計測元。
const WEB_OPEN_EVENT: &str = "web_open";

/// React の単一ページの入口のファイル。画面の URL (`/db/item` 等) はファイルではないので、これを返す。
const INDEX_HTML: &str = "index.html";

/// ビルド時に埋め込んだ `web/dist` のファイル。(`web/dist` からの相対パス, 中身)。`web/dist` 無しでビルドした時は空 (`build.rs`)。
static WEB_ASSETS: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// `tansu serve` の引数。
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// 待ち受けるアドレス (`--host`)。
    pub host: String,
    /// 待ち受けるポート (`--port` / `TANSU_PORT`)。
    pub port: u16,
    /// `Host` の許可リストに足すホスト名 (`--allow-host`。Tailscale の MagicDNS 名など)。
    pub allow_hosts: Vec<String>,
}

/// `options` で待ち受け、Web 画面と HTTP API を配信する。止められるまで戻らない。
/// 起動時に DB を開いて (マイグレーションを当てて) からポートを確保し、HTTP API のトークンを作って `$TANSU_HOME/token` に書き
/// ([`api_token::issue`])、リクエストの処理を始める。設定の誤りを最初のリクエストより前に返し、最初のリクエストを処理する時には
/// トークンのファイルが書き終わっている (`make` と E2E は応答が返ってからファイルを読む)。トークンをポートの確保の後に書くのは、
/// 同じポートで動いている serve がある時に bind で失敗した起動が、動いている serve のトークンのファイルを上書きしないため。
pub async fn serve(settings: Settings, options: ServeOptions) -> Result<(), Error> {
    let interface_ips: Vec<IpAddr> = if is_unspecified(&options.host) {
        if_addrs::get_if_addrs()?
            .iter()
            .map(if_addrs::Interface::ip)
            .collect()
    } else {
        Vec::new()
    };
    db::open(&settings).await?;
    let listener = tokio::net::TcpListener::bind((host_name(&options.host), options.port)).await?;
    let local_addr = listener.local_addr()?;
    let token = api_token::issue(&settings.home)?;
    // トークンは stderr にだけ出す。同じ LAN の iPhone から開く時は、この URL のホスト部をこの Mac の IP に変えて開く
    eprintln!(
        "tansu: http://{local_addr}/ で待ち受けています。画面は http://{local_addr}/#token={token} で開きます"
    );
    // `--port 0` の時は OS が選んだポートで待ち受けるため、`Host` の検査には実際のポートを使う
    let options = ServeOptions {
        port: local_addr.port(),
        ..options
    };
    axum::serve(listener, router(settings, &options, &interface_ips, token)).await?;
    Ok(())
}

/// Web 画面と HTTP API の router。`interface_ips` は `options.host` が `0.0.0.0` / `::` の時に `Host` の許可リストへ足す、
/// このマシンのネットワークインターフェースの IP (それ以外の時は使わない)。`token` は HTTP API が
/// `Authorization: Bearer` で受け付けるトークン ([`api_token::issue`] が作ったもの)。
pub fn router(
    settings: Settings,
    options: &ServeOptions,
    interface_ips: &[IpAddr],
    token: String,
) -> Router {
    let guard = Arc::new(Guard {
        allowed_hosts: allowed_hosts(options, interface_ips),
        port: options.port,
        token,
    });
    Router::new()
        .route("/api/databases", get(list_databases))
        .route(
            "/api/databases/{database}/rows",
            get(database_rows).post(upsert_row),
        )
        .route("/api/databases/{database}/schema", post(add_properties))
        .route("/api/qa/session", get(qa_session))
        .route("/api/qa/record", post(qa_record))
        .route("/api/usage/web-open", post(record_web_open))
        .fallback(web_asset)
        .with_state(AppState { settings })
        .layer(middleware::from_fn_with_state(guard, check_request))
}

/// HTTP API の handler が共有する状態。
#[derive(Debug, Clone)]
struct AppState {
    /// DB と利用記録の置き場所。リクエストごとにここから DB を開く (CLI と同じく、接続をリクエストの間で共有しない)。
    settings: Settings,
}

/// `Host`・トークン・書き込みのリクエストの検査に使う、待ち受けの情報。
struct Guard {
    /// `Host` のホスト部として受け付ける値 (小文字。IPv6 は `[` `]` を除く)。
    allowed_hosts: BTreeSet<String>,
    /// 待ち受けるポート。`Host` のポート (無ければ HTTP の既定の 80) はこれと一致しなければならない。
    port: u16,
    /// HTTP API が `Authorization: Bearer` で受け付けるトークン。
    token: String,
}

/// HTTP API のパスの接頭辞。これで始まるリクエストはトークンを要る。画面のファイル (データを含まない) は要らない。
const API_PATH_PREFIX: &str = "/api/";

/// `Authorization` の認証方式のうち、トークンを送るもの (RFC 6750 2.1)。
const BEARER_SCHEME: &str = "Bearer";

/// トークンが無い・違う API のリクエストへの 401 の理由。画面はこれをそのまま表示する。
const UNAUTHORIZED_REASON: &str =
    "トークンがありません。tansu serve が表示した URL (#token= の付いたもの) から開き直してください";

/// `Host` のポートが省略された時のポート (HTTP の既定。RFC 9110 4.2.1)。
const HTTP_DEFAULT_PORT: u16 = 80;

impl Guard {
    /// `Host` の値 `host` を受け付けるか。
    fn allows(&self, host: &str) -> bool {
        split_host(host).is_some_and(|(name, port)| {
            self.allowed_hosts.contains(&name) && port.unwrap_or(HTTP_DEFAULT_PORT) == self.port
        })
    }

    /// `headers` の `Authorization` が `Bearer <トークン>` で、トークンが [`Guard::token`] と同じか。認証方式の名前は
    /// 大文字・小文字を区別しない (RFC 9110 11.1)。
    fn authorizes(&self, headers: &HeaderMap) -> bool {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|authorization| authorization.to_str().ok())
            .and_then(|authorization| authorization.split_once(' '))
            .is_some_and(|(scheme, token)| {
                scheme.eq_ignore_ascii_case(BEARER_SCHEME)
                    && constant_time_eq(token.trim().as_bytes(), self.token.as_bytes())
            })
    }
}

/// `left` と `right` が同じバイト列か。長さが同じ時は途中で違っても最後まで比べ、何文字目まで一致したかを応答の時間から
/// 推し量らせない (トークンの比較に使う)。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .map(|(left_byte, right_byte)| left_byte ^ right_byte)
            .fold(0, |difference, bits| difference | bits)
            == 0
}

/// `Host` の許可リスト (`documents/PROJECT.md`「制約」)。`127.0.0.1` と `localhost`、`--host` の値、`--allow-host` の値に加え、
/// `--host` が `0.0.0.0` / `::` の時だけ `interface_ips` (このマシンのネットワークインターフェースの IP)。
fn allowed_hosts(options: &ServeOptions, interface_ips: &[IpAddr]) -> BTreeSet<String> {
    LOOPBACK_HOSTS
        .iter()
        .map(|host| (*host).to_owned())
        .chain(std::iter::once(
            host_name(&options.host).to_ascii_lowercase(),
        ))
        .chain(
            options
                .allow_hosts
                .iter()
                .map(|host| host_name(host).to_ascii_lowercase()),
        )
        .chain(
            interface_ips
                .iter()
                .filter(|_| is_unspecified(&options.host))
                .map(IpAddr::to_string),
        )
        .collect()
}

/// `--host` / `--allow-host` の値から、IPv6 のアドレスを囲む `[` `]` を除いたもの。
fn host_name(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
}

/// `host` が、すべてのインターフェースで待ち受けるアドレス (`0.0.0.0` / `::`) か。
fn is_unspecified(host: &str) -> bool {
    host_name(host)
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_unspecified())
}

/// `Host` の値を (小文字のホスト部, ポート) に分ける。IPv6 は `[::1]:7979` の形で、ホスト部は `::1`。
/// ポートが無ければ `None`。ポートが数でない・`[` が閉じていない値は読めないので `None`。
fn split_host(host: &str) -> Option<(String, Option<u16>)> {
    let (name, port) = match host.strip_prefix('[') {
        Some(rest) => {
            let (name, after) = rest.split_once(']')?;
            match after {
                "" => (name, None),
                after => (name, Some(after.strip_prefix(':')?)),
            }
        }
        None => match host.rsplit_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        },
    };
    let port = match port {
        Some(port) => Some(port.parse::<u16>().ok()?),
        None => None,
    };
    Some((name.to_ascii_lowercase(), port))
}

/// すべてのリクエストの前に置く検査。`Host` が許可リストに無ければ 403 (DNS rebinding 対策)。HTTP API
/// ([`API_PATH_PREFIX`]) は、トークンが無い・違うと 401 (同じマシンの別のユーザー・プロセスに読み書きさせない)。GET / HEAD 以外
/// (書き込み) は、別のサイトからの form POST・no-cors のリクエストを通さないよう [`write_rejection`] でも検査する。
async fn check_request(State(guard): State<Arc<Guard>>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .filter(|host| guard.allows(host));
    let rejection = match host {
        None => Some((
            StatusCode::FORBIDDEN,
            "Host が許可されていません (tansu serve --allow-host <ホスト名> で足せます)",
        )),
        Some(_)
            if request.uri().path().starts_with(API_PATH_PREFIX)
                && !guard.authorizes(request.headers()) =>
        {
            return (
                [(header::WWW_AUTHENTICATE, BEARER_SCHEME)],
                error_response(StatusCode::UNAUTHORIZED, UNAUTHORIZED_REASON),
            )
                .into_response();
        }
        Some(_) if matches!(*request.method(), Method::GET | Method::HEAD) => None,
        Some(host) => write_rejection(request.headers(), host),
    };
    match rejection {
        Some((status, reason)) => error_response(status, reason),
        None => next.run(request).await,
    }
}

/// 書き込みのリクエストを拒否する理由。`Sec-Fetch-Site` が `cross-site`、`Origin` が `http://<Host>` と違う、
/// 本文が `Content-Type: application/json` でない (別のサイトがプリフライトなしで送れる形式) のどれかなら拒否する。
/// `Origin` が無いリクエスト (curl 等) は受け付ける。
fn write_rejection(headers: &HeaderMap, host: &str) -> Option<(StatusCode, &'static str)> {
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site.as_bytes().eq_ignore_ascii_case(b"cross-site"))
    {
        return Some((
            StatusCode::FORBIDDEN,
            "別のサイトからの書き込みは受け付けません",
        ));
    }
    if headers.get(header::ORIGIN).is_some_and(|origin| {
        !origin
            .to_str()
            .is_ok_and(|origin| origin.eq_ignore_ascii_case(&format!("http://{host}")))
    }) {
        return Some((
            StatusCode::FORBIDDEN,
            "Origin が Host と違う書き込みは受け付けません",
        ));
    }
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|content_type| content_type.to_str().ok())
        .and_then(|content_type| content_type.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"));
    (!is_json).then_some((
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "書き込みの本文は Content-Type: application/json で送ってください",
    ))
}

/// `{"error": <理由>}` の本文のエラーの応答。
fn error_response(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({"error": reason}))).into_response()
}

/// HTTP API が返すエラー。コアのエラーを状態コードと `{"error": <理由>}` の本文にする。
struct ApiError(Error);

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            Error::DatabaseNotFound(_) | Error::PropertyNotFound { .. } => StatusCode::NOT_FOUND,
            Error::InvalidInput(_) | Error::UniqueViolation { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        error_response(status, &self.0.to_string())
    }
}

/// `GET /api/databases`: すべてのデータベースの定義 (`tansu db list` と同じ形)。
async fn list_databases(State(state): State<AppState>) -> Result<Json<Vec<Database>>, ApiError> {
    Ok(Json(
        schema::list_databases(&db::open(&state.settings).await?).await?,
    ))
}

/// `GET /api/databases/{database}/rows`: データベースの定義とすべての行 (`tansu export --format json` と同じ形)。
/// 絞り込みと並べ替えは画面が行う (select・relation の並べ替えと部分一致の絞り込みは `tansu query` に無いため)。
async fn database_rows(
    State(state): State<AppState>,
    Path(database_name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (database, rows) = rows::query(
        &db::open(&state.settings).await?,
        &database_name,
        &[],
        None,
        None,
    )
    .await?;
    Ok(Json(export::to_json(&database, &rows)))
}

/// `POST /api/databases/{database}/rows`: 行を書き、書いた行を返す (`tansu upsert` と同じ入出力)。
/// project の `path` を書く入力は拒否する (`qa import` が書いた checkout のパスだけを `git -C` に渡すため)。
async fn upsert_row(
    State(state): State<AppState>,
    Path(database_name): Path<String>,
    Json(input): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if database_name == qa::PROJECT
        && input
            .get("values")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|values| values.contains_key(qa::PROJECT_PATH))
    {
        return Err(Error::InvalidInput(format!(
            "{} の {} は qa import だけが書きます (画面と HTTP API からは書けません)",
            qa::PROJECT,
            qa::PROJECT_PATH
        ))
        .into());
    }
    let (database, row) =
        rows::upsert(&db::open(&state.settings).await?, &database_name, &input).await?;
    Ok(Json(named_json::row_to_json(&database, &row)))
}

/// `POST /api/databases/{database}/schema` の本文。画面からはプロパティと選択肢を足すだけにし、unique にする・外す
/// (行の値も消える) 操作は CLI の `tansu db schema` に残す。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaAddition {
    /// `tansu db schema --add` と同じ指定 (`<プロパティ名>:<型>[:<選択肢,...>|:<相手の表>]`)。
    add: Vec<String>,
}

/// `POST /api/databases/{database}/schema`: プロパティ・選択肢を足し、変えた後の定義を返す。
async fn add_properties(
    State(state): State<AppState>,
    Path(database_name): Path<String>,
    Json(addition): Json<SchemaAddition>,
) -> Result<Json<Database>, ApiError> {
    Ok(Json(
        schema::change_schema(
            &db::open(&state.settings).await?,
            &database_name,
            &addition.add,
            &[],
            &[],
        )
        .await?,
    ))
}

/// `GET /api/qa/session` の問い合わせ。
#[derive(Debug, Deserialize)]
struct SessionQuery {
    /// project の `owner/repo`。
    repo: String,
    /// feature 名。
    feature: String,
}

/// `GET /api/qa/session?repo=<owner/repo>&feature=<feature 名>`: QA の入力ビューの内容。
async fn qa_session(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<InputSession>, ApiError> {
    Ok(Json(
        input::session(
            &db::open(&state.settings).await?,
            &query.repo,
            &query.feature,
        )
        .await?,
    ))
}

/// `POST /api/qa/record` の本文。`tansu qa record --item` の引数のうち、画面が入力するもの。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QaRecordInput {
    /// item の行 ID。
    item_id: String,
    /// `ok` / `ng` / `later` / `skip`。
    result: String,
    /// 検証した commit の full SHA (入力ビューが表示した project の HEAD)。
    commit: String,
    /// メモ。
    note: Option<String>,
    /// 検証環境。
    env: Option<String>,
    /// 冪等キー。同じ値の run は 1 行にする (`documents/cli-contract.md`「tansu qa record」)。
    run_id: Option<String>,
}

/// `POST /api/qa/record`: run を 1 行書き (`tansu qa record` と同じ)、書いた run の行を返す。
async fn qa_record(
    State(state): State<AppState>,
    Json(input): Json<QaRecordInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (database, row) = record::record(
        &db::open(&state.settings).await?,
        &RecordRequest {
            item: ItemSelector::Id(input.item_id),
            result: input.result,
            commit: input.commit,
            note: input.note,
            image_urls: Vec::new(),
            env: input.env,
            run_id: input.run_id,
        },
    )
    .await?;
    Ok(Json(named_json::row_to_json(&database, &row)))
}

/// `POST /api/usage/web-open`: 画面を読み込んだことを利用記録に 1 行書く。書き込みなので CSRF の検査を通す。
async fn record_web_open(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    usage::record(&state.settings.home, WEB_OPEN_EVENT)?;
    Ok(StatusCode::NO_CONTENT)
}

/// API 以外の GET: 埋め込んだ `web/dist` のファイル。画面の URL ([`is_page_path`]) は `index.html` を返す。
async fn web_asset(method: Method, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if !matches!(method, Method::GET | Method::HEAD) || path.starts_with("api/") {
        return error_response(StatusCode::NOT_FOUND, "この URL はありません");
    }
    let asset = if is_page_path(path) {
        find_asset(INDEX_HTML)
    } else {
        find_asset(path)
    };
    match asset {
        Some((asset_path, contents)) => {
            ([(header::CONTENT_TYPE, content_type(asset_path))], *contents).into_response()
        }
        None if WEB_ASSETS.is_empty() => (
            StatusCode::SERVICE_UNAVAILABLE,
            "この tansu は Web 画面 (web/dist) を埋め込まずにビルドされています。make cli でビルドし直してください",
        )
            .into_response(),
        None => error_response(StatusCode::NOT_FOUND, "この URL はありません"),
    }
}

/// 先頭の `/` を除いたパス `path` が画面の URL (`web/src/route.ts` の `parseRoute` が読むもの) か。データベース名は
/// `.` を含めるため、拡張子の有無ではなく画面のルートで見分ける。
fn is_page_path(path: &str) -> bool {
    path.is_empty() || path == "qa" || path == "db" || path.starts_with("db/")
}

/// 埋め込んだファイルのうち、パスが `path` のもの。
fn find_asset(path: &str) -> Option<&'static (&'static str, &'static [u8])> {
    WEB_ASSETS
        .iter()
        .find(|(asset_path, _)| *asset_path == path)
}

/// 埋め込んだファイルの `Content-Type`。Vite の成果物に出る拡張子だけを見て、それ以外はバイナリとして返す。
fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn options(host: &str, allow_hosts: &[&str]) -> ServeOptions {
        ServeOptions {
            host: host.into(),
            port: DEFAULT_PORT,
            allow_hosts: allow_hosts.iter().map(|host| (*host).to_owned()).collect(),
        }
    }

    fn lan_ips() -> Vec<IpAddr> {
        vec!["192.168.1.20".parse().unwrap(), "fe80::1".parse().unwrap()]
    }

    #[test]
    fn default_host_allows_only_this_machine() {
        assert_eq!(
            allowed_hosts(&options(DEFAULT_HOST, &[]), &lan_ips()),
            BTreeSet::from(["127.0.0.1".to_owned(), "localhost".to_owned()])
        );
    }

    #[test]
    fn unspecified_host_allows_interface_ips() {
        for host in ["0.0.0.0", "::", "[::]"] {
            let allowed = allowed_hosts(&options(host, &[]), &lan_ips());
            assert!(allowed.contains("192.168.1.20"), "{host}: {allowed:?}");
            assert!(allowed.contains("fe80::1"), "{host}: {allowed:?}");
        }
    }

    #[test]
    fn allow_host_and_host_are_added_in_lowercase() {
        let allowed = allowed_hosts(&options("192.168.1.20", &["Mac.Tailnet.ts.net"]), &[]);
        assert!(allowed.contains("192.168.1.20"));
        assert!(allowed.contains("mac.tailnet.ts.net"));
    }

    #[test]
    fn host_header_is_split_into_name_and_port() {
        assert_eq!(
            split_host("LocalHost:7979"),
            Some(("localhost".into(), Some(7979)))
        );
        assert_eq!(split_host("127.0.0.1"), Some(("127.0.0.1".into(), None)));
        assert_eq!(split_host("[::1]:7979"), Some(("::1".into(), Some(7979))));
        assert_eq!(split_host("[::1]"), Some(("::1".into(), None)));
        for invalid in ["[::1", "[::1]x", "localhost:port", "localhost:99999"] {
            assert_eq!(split_host(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn guard_requires_the_listening_port() {
        let guard = Guard {
            allowed_hosts: allowed_hosts(&options(DEFAULT_HOST, &[]), &[]),
            port: DEFAULT_PORT,
            token: "secret".into(),
        };
        assert!(guard.allows("127.0.0.1:7979"));
        assert!(guard.allows("localhost:7979"));
        assert!(!guard.allows("127.0.0.1:8080"));
        assert!(!guard.allows("127.0.0.1"));
        assert!(!guard.allows("evil.example:7979"));
    }

    #[test]
    fn guard_authorizes_only_the_bearer_token() {
        let guard = Guard {
            allowed_hosts: BTreeSet::new(),
            port: DEFAULT_PORT,
            token: "secret".into(),
        };
        let authorization = |value: &'static str| {
            HeaderMap::from_iter([(header::AUTHORIZATION, HeaderValue::from_static(value))])
        };
        assert!(guard.authorizes(&authorization("Bearer secret")));
        assert!(guard.authorizes(&authorization("bearer secret")));
        for invalid in [
            "Bearer secrets",
            "Bearer secre",
            "Bearer ",
            "Basic secret",
            "secret",
        ] {
            assert!(!guard.authorizes(&authorization(invalid)), "{invalid}");
        }
        assert!(!guard.authorizes(&HeaderMap::new()));
    }

    #[test]
    fn constant_time_eq_compares_whole_bytes() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn page_paths_include_database_names_with_dots() {
        for page in ["", "qa", "db/item", "db/tasks.v2", "db/tasks.v2/row_id"] {
            assert!(is_page_path(page), "{page}");
        }
        for asset in ["index.html", "assets/index-abc.js", "favicon.svg"] {
            assert!(!is_page_path(asset), "{asset}");
        }
    }

    #[test]
    fn content_type_follows_the_extension() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type("assets/index-abc.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type("favicon"), "application/octet-stream");
    }
}
