//! Russia/Moscow section of 俄通收RETS新品价格测算表2026.07.24执行.
//! Economy where offered; Big is Standard (the only listed Big service).
//! Rates and formulas agree in G/M rows 6,10,13,17,19,21. CNY fees; RUB bands.
pub(crate) const SALE_BANDS_RUB: [f64;3] = [1500.0,7000.0,250000.0];
pub(crate) const BASE_CNY:[f64;6]=[3.37,17.97,24.71,25.83,40.44,69.64];
pub(crate) const PER_KG_CNY:[f64;6]=[28.1,28.1,28.1,19.1,28.1,25.8];

fn snap(value:f64, boundaries:&[f64]) -> f64 {
    boundaries.iter().copied().find(|b|(value-b).abs()<=f64::EPSILON*8.0*b.abs().max(1.0)).unwrap_or(value)
}

#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Quote { pub cny:f64, pub billed_kg:f64, pub tier:usize }

pub(crate) fn quote(price_rub:f64, physical_kg:f64, _dimensions_cm:Option<[f64;3]>) -> Result<Quote,String> {
    if !price_rub.is_finite() || !physical_kg.is_finite() { return Err("售价或重量无效".into()); }
    let price=snap(price_rub,&[1.0,1500.0,7000.0,250000.0]);
    let weight=snap(physical_kg,&[0.5,2.0,5.0,30.0]);
    if !(1.0..=250000.0).contains(&price) { return Err("RETS 俄罗斯段支持货值 1～250000 RUB".into()); }
    if weight<=0.0 || weight>30.0 { return Err("RETS 俄罗斯段实重必须大于 0 且不超过 30kg".into()); }
    // Source bands are inclusive at 500/2000/5000/30000 g. No 0.1 kg rounding.
    let tier=if price<=1500.0 {if weight<=0.5 {0}else{3}}
        else if price<=7000.0 {if weight<=2.0 {1}else{4}}
        else if weight<=5.0 {2}else{5};
    // User-selected estimate policy: physical weight only; dimensions do not gate or select tiers.
    // The authoritative Russia formulas all use R4 directly (physical kg).
    let billed_kg=weight;
    let base=BASE_CNY[tier];
    let per_kg=PER_KG_CNY[tier];
    Ok(Quote {cny:((base+billed_kg*per_kg)*100.).round()/100.,billed_kg,tier})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracted_source_rate_text_and_source_formulas_agree_with_model() {
        let rows:serde_json::Value=serde_json::from_str(include_str!("fixtures/rets_russia_20260724.json")).unwrap();
        for row in rows.as_array().unwrap() {
            let i=row["tier"].as_u64().unwrap() as usize;
            assert_eq!(BASE_CNY[i],row["base_cny"].as_f64().unwrap());
            assert_eq!(PER_KG_CNY[i],row["per_kg_cny"].as_f64().unwrap());
            let q=quote(row["price_rub"].as_f64().unwrap(),row["weight_kg"].as_f64().unwrap(),Some([10.;3])).unwrap();
            assert_eq!(q.cny,row["expected_cny"].as_f64().unwrap(),"source row {}",row["source_row"]);
        }
    }
    fn fee(p:f64,w:f64)->Option<f64> {quote(p,w,Some([10.;3])).ok().map(|q|q.cny)}
    #[test]
    fn all_six_economy_tiers_match_source_cny_per_gram_prices() {
        for (p,w,expected) in [(1000.,0.4,14.61),(2000.,1.,46.07),(8000.,3.,109.01),(1000.,1.,44.93),(2000.,3.,124.74),(8000.,6.,224.44)] {assert_eq!(fee(p,w),Some(expected));}
    }
    #[test]
    fn source_price_and_weight_boundaries_are_inclusive() {
        assert_eq!(fee(1500.,0.5),Some(17.42)); assert_eq!(fee(1500.01,0.5),Some(32.02));
        assert_eq!(fee(7000.,2.),Some(74.17)); assert_eq!(fee(7000.01,2.),Some(80.91));
        assert_eq!(fee(250000.,5.),Some(165.21)); assert_eq!(fee(250000.01,5.),None);
        assert!(fee(1000.,30.).is_some()); assert!(fee(1000.,30.00001).is_none());
        for (p,b) in [(1000.,0.5),(2000.,2.),(8000.,5.)] {
            assert_eq!(fee(p,b-f64::EPSILON),fee(p,b));
            assert_eq!(fee(p,b+f64::EPSILON),fee(p,b));
            assert_ne!(fee(p,b+0.00001),fee(p,b));
        }
        assert_eq!(fee(1000.,0.301),Some(11.83)); assert_eq!(fee(1000.,0.401),Some(14.64));
    }
    #[test]
    fn physical_weight_only_ignores_missing_or_oversize_dimensions() {
        let q=quote(2000.,3.,Some([60.,40.,40.])).unwrap();
        assert_eq!(q.billed_kg,3.); assert_eq!(q.tier,4); assert_eq!(q.cny,124.74);
        assert_eq!(quote(1000.,0.4,None).unwrap().cny,14.61);
        assert_eq!(quote(1000.,0.4,Some([999.,999.,999.])).unwrap().cny,14.61);
        assert_eq!(quote(1000.,0.5,Some([50.,30.,20.])).unwrap().tier,0);
        assert_eq!(quote(2000.,2.,Some([80.,20.,20.])).unwrap().tier,1);
    }
}

