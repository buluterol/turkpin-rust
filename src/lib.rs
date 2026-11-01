//! Async client for the Turkpin dealer (bayi) API.
//!
//! Every call is an HTTP POST to api.php with one form field, `DATA`, holding
//! `<APIRequest><params>...</params></APIRequest>`. Responses are XML. Docs: <https://dev.turkpin.com>
//!
//! Money values are kept as strings so no precision is lost; parse them with the decimal type you use.

use std::fmt;
use std::time::Duration;

pub const LIVE_URL: &str = "https://www.turkpin.com/api.php";
pub const TEST_URL: &str = "https://www.turkpin.net/api.php";

/// Values of `SIPARIS_DURUMU` returned by siparisDurumu and siparisListesi.
pub mod order_status {
    pub const PROCESSING: i32 = 1;
    pub const COMPLETED: i32 = 2;
    pub const CANCELLED: i32 = 3;
    pub const DELIVERING: i32 = 99;
    pub const PRE_ORDER_DELIVERING: i32 = 199;
}

#[derive(Debug)]
pub enum Error {
    /// Error code other than "000" returned by the API, e.g. "012".
    Api { code: String, message: String },
    /// Transport failure.
    Http(reqwest::Error),
    /// Unexpected HTTP status.
    Status(u16),
    /// Response is not the expected XML.
    InvalidResponse(String),
}

impl Error {
    /// API error code; None for HTTP / parse errors.
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Api { code, message } => write!(f, "turkpin: {message} ({code})"),
            Error::Http(e) => write!(f, "turkpin: HTTP request failed: {e}"),
            Error::Status(s) => write!(f, "turkpin: HTTP {s} from Turkpin API"),
            Error::InvalidResponse(r) => write!(f, "turkpin: invalid response: {r}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Http(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq)]
pub struct Game {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub stock: i64,
    pub min_order: i64,
    /// 0 means no upper limit.
    pub max_order: i64,
    pub price: String,
    pub tax_type: String,
    pub pre_order: bool,
    /// Tiered (barem) products only.
    pub min_barem: Option<String>,
    pub max_barem: Option<String>,
    pub barem_step: Option<String>,
}

impl Product {
    pub fn is_tiered(&self) -> bool {
        self.min_barem.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Epin {
    pub code: String,
    pub description: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub order_no: String,
    /// "Success" or "Pending".
    pub result: String,
    pub amount: String,
    pub epins: Vec<Epin>,
}

impl Order {
    /// True for pre-order / tiered orders: poll `order_status` for the codes.
    pub fn is_pending(&self) -> bool {
        self.result != "Success"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderInfo {
    pub order_no: String,
    /// One of the `order_status` constants.
    pub status: i32,
    pub description: String,
    /// "000" normally, "999" when Turkpin is handling a problem with the order.
    pub state_code: String,
    pub checked_at: String,
    pub amount: String,
    /// Cancellation reason or problem note.
    pub extra: String,
    pub epins: Vec<Epin>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    pub balance: String,
    pub credit: String,
    pub bonus: String,
    pub spending: String,
}

/// Parameters of `create_order`. `pre_order` must be true for products listed with `pre_order`;
/// `barem` is the amount for tiered products (between min_barem and max_barem, in barem_step steps).
#[derive(Debug, Clone, Default)]
pub struct OrderRequest {
    pub game_id: i64,
    pub product_id: i64,
    /// Defaults to 1 when 0.
    pub quantity: i64,
    pub character: Option<String>,
    pub pre_order: bool,
    pub barem: Option<String>,
}

/// Turkpin API client. Cheap to clone (the HTTP connection pool is shared), so one instance can be
/// stored in application state (Axum, Actix Web, ...).
#[derive(Debug, Clone)]
pub struct Client {
    username: String,
    password: String,
    url: String,
    http: reqwest::Client,
}

impl Client {
    /// Client for the live system, or the sandbox when `test` is true.
    pub fn new(username: impl Into<String>, password: impl Into<String>, test: bool) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self::with_http_client(
            username,
            password,
            if test { TEST_URL } else { LIVE_URL },
            http,
        )
    }

    /// Client with a custom endpoint and HTTP client (timeouts, proxy, tests).
    pub fn with_http_client(
        username: impl Into<String>,
        password: impl Into<String>,
        url: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            url: url.into(),
            http,
        }
    }

    /// Builds a client from TURKPIN_USERNAME, TURKPIN_PASSWORD and TURKPIN_TEST (1/true).
    /// Returns None when the username or password is missing.
    pub fn from_env() -> Option<Self> {
        let username = std::env::var("TURKPIN_USERNAME")
            .ok()
            .filter(|s| !s.is_empty())?;
        let password = std::env::var("TURKPIN_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty())?;
        let test = std::env::var("TURKPIN_TEST")
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        Some(Self::new(username, password, test))
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub async fn games(&self) -> Result<Vec<Game>> {
        let xml = self.call("epinOyunListesi", &[]).await?;
        let doc = parse(&xml)?;
        let p = params(&doc)?;
        Ok(items(child(Some(p), "oyunListesi"), "oyun")
            .map(|g| Game {
                id: int(text(Some(g), "id")),
                name: text(Some(g), "name"),
            })
            .collect())
    }

    /// Lists e-pin products of a game; `product_id` None lists all of them.
    pub async fn products(&self, game_id: i64, product_id: Option<i64>) -> Result<Vec<Product>> {
        let mut args = vec![("oyunKodu", game_id.to_string())];
        if let Some(id) = product_id {
            args.push(("urunKodu", id.to_string()));
        }
        let xml = self.call("epinUrunleri", &args).await?;
        let doc = parse(&xml)?;
        let p = params(&doc)?;
        Ok(items(child(Some(p), "epinUrunListesi"), "urun")
            .map(|u| {
                let u = Some(u);
                let opt = |name| Some(text(u, name)).filter(|s| !s.is_empty());
                Product {
                    id: int(text(u, "id")),
                    name: text(u, "name"),
                    stock: int(text(u, "stock")),
                    min_order: int(text(u, "min_order")).max(1),
                    max_order: int(text(u, "max_order")),
                    price: text(u, "price"),
                    tax_type: text(u, "tax_type"),
                    pre_order: text(u, "pre_order").eq_ignore_ascii_case("true"),
                    min_barem: opt("min_barem"),
                    max_barem: opt("max_barem"),
                    barem_step: opt("barem_step"),
                }
            })
            .collect())
    }

    pub async fn create_order(&self, r: OrderRequest) -> Result<Order> {
        let quantity = if r.quantity == 0 { 1 } else { r.quantity };
        let mut args = vec![
            ("oyunKodu", r.game_id.to_string()),
            ("urunKodu", r.product_id.to_string()),
            ("adet", quantity.to_string()),
        ];
        if let Some(c) = r.character {
            args.push(("character", c));
        }
        if r.pre_order {
            args.push(("pre_order", "true".into()));
        }
        if let Some(b) = r.barem {
            args.push(("barem", b));
        }
        let xml = self.call("epinSiparisYarat", &args).await?;
        let doc = parse(&xml)?;
        let p = Some(params(&doc)?);
        Ok(Order {
            order_no: text(p, "siparisNo"),
            result: text(p, "siparisSonuc"),
            amount: text(p, "siparisTutari"),
            epins: epins(p),
        })
    }

    pub async fn order_status(&self, order_no: &str) -> Result<OrderInfo> {
        let xml = self
            .call("siparisDurumu", &[("siparisNo", order_no.to_string())])
            .await?;
        let doc = parse(&xml)?;
        Ok(order_info(params(&doc)?))
    }

    /// Orders between two dates (YYYY-MM-DD, inclusive, at most 30 days apart).
    pub async fn orders(&self, start: &str, end: &str) -> Result<Vec<OrderInfo>> {
        let args = [
            ("baslangicTarihi", start.to_string()),
            ("bitisTarihi", end.to_string()),
        ];
        let xml = self.call("siparisListesi", &args).await?;
        let doc = parse(&xml)?;
        let p = params(&doc)?;
        Ok(items(child(Some(p), "SIPARISLER"), "SIPARIS")
            .map(order_info)
            .collect())
    }

    pub async fn balance(&self) -> Result<Balance> {
        let xml = self.call("balance", &[]).await?;
        let doc = parse(&xml)?;
        let b = child(Some(params(&doc)?), "balanceInformation");
        Ok(Balance {
            balance: text(b, "balance"),
            credit: text(b, "credit"),
            bonus: text(b, "bonus"),
            spending: text(b, "spending"),
        })
    }

    /// Sends one command and returns the raw XML response after checking the error code.
    pub async fn call(&self, cmd: &str, args: &[(&str, String)]) -> Result<String> {
        let mut xml = String::from("<APIRequest><params>");
        let base = [
            ("cmd", cmd),
            ("username", self.username.as_str()),
            ("password", self.password.as_str()),
        ];
        for (k, v) in base
            .iter()
            .copied()
            .chain(args.iter().map(|(k, v)| (*k, v.as_str())))
        {
            xml.push_str(&format!("<{k}>{}</{k}>", escape(v)));
        }
        xml.push_str("</params></APIRequest>");

        let res = self
            .http
            .post(&self.url)
            .form(&[("DATA", xml)])
            .send()
            .await?;
        let status = res.status().as_u16();
        let body = res.text().await?;
        if status >= 400 && status != 401 {
            return Err(Error::Status(status));
        }
        let doc = parse(&body)?;
        let p = Some(params(&doc)?);
        let mut code = text(p, "error");
        if code.is_empty() {
            code = text(p, "HATA_NO");
        }
        if !code.is_empty() && code != "000" {
            let mut message = text(p, "error_desc");
            if message.is_empty() {
                message = text(p, "HATA_ACIKLAMA");
            }
            if message.is_empty() {
                message = "Turkpin API error".into();
            }
            return Err(Error::Api { code, message });
        }
        Ok(body)
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse(body: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse(body.trim())
        .map_err(|_| Error::InvalidResponse(body.chars().take(200).collect()))
}

fn params<'a, 'i>(doc: &'a roxmltree::Document<'i>) -> Result<roxmltree::Node<'a, 'i>> {
    child(Some(doc.root_element()), "params")
        .ok_or_else(|| Error::InvalidResponse("missing <params>".into()))
}

fn child<'a, 'i>(
    n: Option<roxmltree::Node<'a, 'i>>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'i>> {
    n?.children()
        .find(|c| c.is_element() && c.has_tag_name(name))
}

fn items<'a, 'i: 'a>(
    n: Option<roxmltree::Node<'a, 'i>>,
    name: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> + 'a {
    n.into_iter()
        .flat_map(|n| n.children())
        .filter(move |c| c.is_element() && c.has_tag_name(name))
}

fn text(n: Option<roxmltree::Node>, name: &str) -> String {
    child(n, name)
        .map(|c| c.text().unwrap_or("").trim().to_string())
        .unwrap_or_default()
}

fn int(s: String) -> i64 {
    s.parse().unwrap_or(0)
}

fn epins(n: Option<roxmltree::Node>) -> Vec<Epin> {
    items(child(n, "epin_list"), "epin")
        .map(|e| Epin {
            code: text(Some(e), "code"),
            description: text(Some(e), "desc"),
            id: text(Some(e), "id"),
        })
        .collect()
}

fn order_info(s: roxmltree::Node) -> OrderInfo {
    let s = Some(s);
    let mut extra = text(s, "EKSTRA");
    if extra.is_empty() {
        extra = text(s, "EXTRA");
    }
    OrderInfo {
        order_no: text(s, "SIPARIS_NO"),
        status: int(text(s, "SIPARIS_DURUMU")) as i32,
        description: text(s, "SIPARIS_DURUMU_ACIKLAMA"),
        state_code: text(s, "DURUM_KODU"),
        checked_at: text(s, "KONTROL_TARIHI"),
        amount: text(s, "SIPARIS_TUTARI"),
        extra,
        epins: epins(s),
    }
}
