# turkpin-rust

[![test](https://github.com/buluterol/turkpin-rust/actions/workflows/test.yml/badge.svg)](https://github.com/buluterol/turkpin-rust/actions/workflows/test.yml)

Turkpin bayi API'si için Rust (async) istemcisi: e-pin oyun ve ürün listesi, sipariş verme, sipariş durumu, sipariş listesi ve bakiye.

> Bu paket topluluk tarafından yazılmıştır; Turkpin'in resmi paketi değildir. API'nin güncel ve bağlayıcı tanımı için [dev.turkpin.com](https://dev.turkpin.com) dokümanına bakın.

Diğer diller / Other languages: [PHP](https://github.com/buluterol/turkpin-php) · [Node.js](https://github.com/buluterol/turkpin-node) · [Python](https://github.com/buluterol/turkpin-python) · [Go](https://github.com/buluterol/turkpin-go) · [Java](https://github.com/buluterol/turkpin-java) · [.NET](https://github.com/buluterol/turkpin-dotnet) · [Ruby](https://github.com/buluterol/turkpin-ruby) · [Rust](https://github.com/buluterol/turkpin-rust)

## Gereksinimler

Güncel kararlı Rust, Tokio çalışma zamanı (reqwest + rustls; OpenSSL gerekmez)

## Kurulum

```toml
[dependencies]
turkpin = { git = "https://github.com/buluterol/turkpin-rust" }
```

Paket crates.io'da yayınlı değil; GitHub'dan kurulur.

## Kullanım

```rust
use turkpin::{order_status, Client, Error, OrderRequest};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let tp = Client::from_env().expect("TURKPIN_USERNAME ve TURKPIN_PASSWORD gerekli");
    // ya da: Client::new(kullanici, sifre, true) // true: test ortamı

    let games = tp.games().await?;
    let products = tp.products(games[0].id, None).await?; // None: tüm ürünler

    match tp
        .create_order(OrderRequest { game_id: games[0].id, product_id: products[0].id, quantity: 1, ..Default::default() })
        .await
    {
        Ok(order) => {
            if order.is_pending() {
                let info = tp.order_status(&order.order_no).await?; // info.status == order_status::COMPLETED olunca kodlar gelir
                let _ = info;
            }
            for e in &order.epins {
                println!("{}", e.code);
            }
        }
        Err(Error::Api { code, message }) => eprintln!("Hata {code}: {message}"),
        Err(e) => return Err(e),
    }

    println!("{}", tp.balance().await?.balance); // tutarlar hassasiyet kaybı olmasın diye String
    Ok(())
}
```

## Axum, Actix Web

`Client` ucuz kopyalanır (bağlantı havuzu paylaşılır); uygulama durumuna koyup handler'lardan kullanın.

```rust
// Axum
let app = Router::new()
    .route("/balance", get(|State(tp): State<turkpin::Client>| async move {
        tp.balance().await.map(|b| b.balance).map_err(|e| e.to_string())
    }))
    .with_state(turkpin::Client::from_env().unwrap());

// Actix Web
HttpServer::new(move || App::new().app_data(web::Data::new(tp.clone())).service(balance))
```

## Desteklenen komutlar

| API komutu | Açıklama |
|---|---|
| `epinOyunListesi` | Oyun listesi |
| `epinUrunleri` | Bir oyunun e-pin ürünleri (stok, fiyat, min/max adet, ön sipariş, barem) |
| `epinSiparisYarat` | Sipariş verme (karakter adı, ön sipariş ve barem tutarı isteğe bağlı) |
| `siparisDurumu` | Sipariş durumu ve teslim edilen kodlar |
| `siparisListesi` | İki tarih arasındaki siparişler (en çok 30 gün) |
| `balance` | Bakiye, kredi, bonus ve harcama |

Listede olmayan komutlar için istemcideki genel `call` metodu kullanılabilir.

**Sipariş durumları (`SIPARIS_DURUMU`):** 1 işleniyor, 2 tamamlandı, 3 iptal, 99 teslimatta, 199 ön sipariş teslimatta. `DURUM_KODU` 999 ise Turkpin siparişle ilgili bir sorunu inceliyor demektir; iptal sebebi ya da not `EKSTRA` alanında gelir.

**Ön sipariş ve barem:** Ürünün `pre_order` alanı `true` ise sipariş ön sipariş olarak verilmelidir. Barem tutarlı ürünlerde tutar `min_barem` ile `max_barem` arasında, `barem_step` adımlarıyla seçilir. Bu siparişlerin sonucu `Pending` döner; kodlar için sipariş durumunu sorgulayın.

**Hatalar:** API `000` dışında bir hata kodu döndürdüğünde istemci, kodu ve açıklamayı taşıyan bir hata fırlatır. HTTP ya da XML hatalarında kod boştur.

## Test ortamı

- Test: `https://www.turkpin.net/api.php`, canlı: `https://www.turkpin.com/api.php`.
- Test ortamına erişmek için sunucu IP adresinizin Turkpin'e tanıtılması gerekir; tanıtılmamış IP'den gelen isteklere API `002` hata koduyla cevap verir. Test hesabı ve IP tanımı için [dev.turkpin.com](https://dev.turkpin.com) adresindeki iletişim bilgilerini kullanın.
- Kullanıcı adı ve şifreyi koda yazmayın; ortam değişkeni ya da gizli anahtar deposundan okuyun.

## Geliştirme

Testler gerçek API'ye bağlanmaz; örnek XML cevaplarıyla yerel sahte sunucuya karşı çalışır.

```sh
cargo test
```

## English

Async Rust client for the Turkpin dealer API (e-pin games, products, orders, order status, order history, balance). Community-maintained, not an official Turkpin package; see [dev.turkpin.com](https://dev.turkpin.com) for the authoritative API reference. The sandbox (`turkpin.net`) only accepts requests from server IP addresses registered with Turkpin (otherwise error `002`). Install and usage are shown above; method names are in English, while API command and field names follow the original Turkish API.

## Lisans

MIT
