use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
fn main() {
    for text in std::env::args().skip(1) {
        let mut h=DefaultHasher::new(); text.hash(&mut h); let mut s=h.finish();
        let mut v=vec![0.0f32;32];
        for x in v.iter_mut(){s=s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); *x=((s>>33) as f32)/(u32::MAX as f32)*2.0-1.0;}
        let n:f32=v.iter().map(|x|x*x).sum::<f32>().sqrt(); for x in v.iter_mut(){*x/=n;}
        println!("{:?}",v);
    }
}
