use std::sync::{Arc, Mutex};
use turkpin::{order_status, Client, Error, OrderRequest, LIVE_URL, TEST_URL};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

struct Recorder {
    body: String,
    sent: Arc<Mutex<Vec<String>>>,
}

impl Respond for Recorder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.sent
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(&req.body).into_owned());
        ResponseTemplate::new(200).set_body_string(self.body.clone())
    }
}

struct Fixture {
    _server: MockServer,
    client: Client,
    sent: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    /// Request XML from the DATA form field of the last call.
    fn last_xml(&self) -> String {
        let form = self.sent.lock().unwrap().last().cloned().unwrap();
        urldecode(form.strip_prefix("DATA=").unwrap())
    }
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].to_string())
}

async fn fixture(file: &str) -> Fixture {
    let body = if file.starts_with('<') {
        file.to_string()
    } else {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{file}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    };
    let server = MockServer::start().await;
    let sent = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("POST"))
        .respond_with(Recorder {
            body,
            sent: sent.clone(),
        })
        .mount(&server)
        .await;
    let client = Client::with_http_client(
        "dealer@example.com",
        "s3cr<et",
        server.uri(),
        reqwest::Client::new(),
    );
    Fixture {
        _server: server,
        client,
        sent,
    }
}

/// Value of a request parameter (still XML-escaped) in the last call.
fn p(f: &Fixture, name: &str) -> Option<String> {
    tag(&f.last_xml(), name)
}

#[tokio::test]
async fn request_format() {
    let f = fixture("games.xml").await;
    f.client.games().await.unwrap();
    assert_eq!(p(&f, "cmd").as_deref(), Some("epinOyunListesi"));
    assert_eq!(p(&f, "password").as_deref(), Some("s3cr&lt;et"));
}

#[test]
fn urls() {
    assert_eq!(Client::new("u", "p", false).url(), LIVE_URL);
    assert_eq!(Client::new("u", "p", true).url(), TEST_URL);
}

#[tokio::test]
async fn games() {
    let f = fixture("games.xml").await;
    let games = f.client.games().await.unwrap();
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].id, 1);
    assert_eq!(games[0].name, "Game 1");
}

#[tokio::test]
async fn products() {
    let f = fixture("products.xml").await;
    let list = f.client.products(1, None).await.unwrap();
    assert_eq!(p(&f, "oyunKodu").as_deref(), Some("1"));
    assert_eq!(p(&f, "urunKodu"), None);
    let (a, b) = (&list[0], &list[1]);
    assert_eq!(a.stock, 26278);
    assert_eq!(a.max_order, 0);
    assert_eq!(a.price, "0.001");
    assert!(!a.pre_order && !a.is_tiered());
    assert!(b.pre_order && b.is_tiered());
    assert_eq!(b.min_barem.as_deref(), Some("25"));
    assert_eq!(b.max_barem.as_deref(), Some("1250"));
    assert_eq!(b.barem_step.as_deref(), Some("0.01"));
    assert_eq!(b.max_order, 5);
}

#[tokio::test]
async fn create_order() {
    let f = fixture("order_success.xml").await;
    let o = f
        .client
        .create_order(OrderRequest {
            game_id: 1,
            product_id: 4,
            quantity: 2,
            character: Some("Player1".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(p(&f, "adet").as_deref(), Some("2"));
    assert_eq!(p(&f, "character").as_deref(), Some("Player1"));
    assert_eq!(p(&f, "pre_order"), None);
    assert_eq!(o.order_no, "25031013105101");
    assert!(!o.is_pending());
    assert_eq!(o.epins[0].code, "S3XS-EEDY-APAE-62EM");
    assert_eq!(o.epins[0].description, "Test Ürün");
}

#[tokio::test]
async fn pre_order_with_barem() {
    let f = fixture("order_pending.xml").await;
    let o = f
        .client
        .create_order(OrderRequest {
            game_id: 1,
            product_id: 4,
            pre_order: true,
            barem: Some("30.50".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(p(&f, "adet").as_deref(), Some("1"));
    assert_eq!(p(&f, "pre_order").as_deref(), Some("true"));
    assert_eq!(p(&f, "barem").as_deref(), Some("30.50"));
    assert!(o.is_pending());
    assert!(o.epins.is_empty());
}

#[tokio::test]
async fn api_errors() {
    let f = fixture("order_out_of_stock.xml").await;
    let e = f
        .client
        .create_order(OrderRequest {
            game_id: 1,
            product_id: 2,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), Some("012"));
    assert!(matches!(&e, Error::Api { message, .. } if message == "Urun stok yetersiz."));
    let f = fixture("error_auth.xml").await;
    assert_eq!(f.client.games().await.unwrap_err().code(), Some("001"));
}

#[tokio::test]
async fn invalid_response() {
    let f = fixture("<html>blocked</html").await;
    assert!(matches!(
        f.client.balance().await,
        Err(Error::InvalidResponse(_))
    ));
}

#[tokio::test]
async fn order_status_completed_and_cancelled() {
    let f = fixture("status_completed.xml").await;
    let s = f.client.order_status("25031013105101").await.unwrap();
    assert_eq!(p(&f, "siparisNo").as_deref(), Some("25031013105101"));
    assert_eq!(s.status, order_status::COMPLETED);
    assert_eq!(s.amount, "0.0010");
    assert_eq!(s.epins[0].code, "S3XS-EEDY-APAE-62EM");
    let f = fixture("status_cancelled.xml").await;
    let c = f.client.order_status("25030620073001").await.unwrap();
    assert_eq!(c.status, order_status::CANCELLED);
    assert_eq!(c.extra, "Stok yok");
}

#[tokio::test]
async fn orders() {
    let f = fixture("orders.xml").await;
    let list = f.client.orders("2026-01-01", "2026-01-30").await.unwrap();
    assert_eq!(p(&f, "baslangicTarihi").as_deref(), Some("2026-01-01"));
    assert_eq!(p(&f, "bitisTarihi").as_deref(), Some("2026-01-30"));
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].status, order_status::PROCESSING);
    assert_eq!(list[1].state_code, "999");
    assert_eq!(list[1].extra, "CANCELLATION REASON");
}

#[tokio::test]
async fn balance() {
    let f = fixture("balance.xml").await;
    let b = f.client.balance().await.unwrap();
    assert_eq!(b.balance, "98689.0000");
    assert_eq!(b.spending, "1311.0000");
}
