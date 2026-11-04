use siren_lib::entity::{AssetDelta, Price};
/// Preserve base-unit evidence; stale, missing and malformed prices stay absent.
pub fn attach_price(
    delta: &mut AssetDelta,
    price: Price,
    now: chrono::DateTime<chrono::Utc>,
    max_age_seconds: i64,
) {
    let age = (now - price.observed_at).num_seconds();
    if age >= 0
        && age <= max_age_seconds
        && price
            .usd_decimal
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
        && !price.usd_decimal.is_empty()
    {
        delta.price = Some(price);
    }
}
