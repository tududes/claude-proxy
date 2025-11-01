/// Determine price tier based on input + output pricing (per million tokens)
pub fn get_price_tier(input_price: Option<f64>, output_price: Option<f64>) -> &'static str {
    let total_price = input_price.unwrap_or(0.0) + output_price.unwrap_or(0.0);
    if input_price.is_none() && output_price.is_none() {
        return "SUB";
    }
    if total_price == 0.0 {
        "SUB"
    } else if total_price <= 0.5 {
        "$"
    } else if total_price <= 2.0 {
        "$$"
    } else if total_price <= 5.0 {
        "$$$"
    } else {
        "$$$$"
    }
}