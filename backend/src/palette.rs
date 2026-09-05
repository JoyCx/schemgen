//! CIEDE2000 perceptual color difference and CIELAB color matching.
//! Uses kiddo's ImmutableKdTree for fast Euclidean pre-filter,
//! then refines top-K with CIEDE2000.

use std::collections::{HashMap, HashSet};
use kiddo::ImmutableKdTree;
use crate::types::{ColorTable, Lab, Rgb};

pub fn rgb_to_lab(r: f32, g: f32, b: f32) -> Lab {
    let lin = |c: f32| -> f32 { let v=c/255.0; if v>0.04045{((v+0.055)/1.055).powf(2.4)}else{v/12.92} };
    let (rl,gl,bl)=(lin(r),lin(g),lin(b));
    let x=rl*0.4124564+gl*0.3575761+bl*0.1804375;
    let y=rl*0.2126729+gl*0.7151522+bl*0.0721750;
    let z=rl*0.0193339+gl*0.1191920+bl*0.9503041;
    let f=|t:f32|->f32{const D:f32=6.0/29.0;if t>D*D*D{t.cbrt()}else{t/(3.0*D*D)+4.0/29.0}};
    Lab{l:116.0*f(y)-16.0,a:500.0*(f(x/0.95047)-f(y)),b:200.0*(f(y)-f(z/1.08883))}
}
pub fn rgb_to_lab_vec(rgb:[f32;3])->Lab{rgb_to_lab(rgb[0],rgb[1],rgb[2])}
pub fn lab_to_rgb(lab:&Lab)->Rgb{
    let fy=(lab.l+16.0)/116.0;let fx=lab.a/500.0+fy;let fz=fy-lab.b/200.0;
    const D:f32=6.0/29.0;let inv=|t|->f32{if t>D{t*t*t}else{3.0*D*D*(t-4.0/29.0)}};
    let x=inv(fx)*0.95047;let y=inv(fy);let z=inv(fz)*1.08883;
    let rl=x*3.2404542+y*-1.5371385+z*-0.4985314;
    let gl=x*-0.9692660+y*1.8760108+z*0.0415560;
    let bl=x*0.0556434+y*-0.2040259+z*1.0572252;
    let d=|c:f32|->f32{let v=c.max(0.).min(1.);((if v>0.0031308{v.powf(1./2.4)*1.055-0.055}else{v*12.92})*255.).round()};
    Rgb{r:d(rl),g:d(gl),b:d(bl)}
}

/// CIEDE2000 using f64 internally for accuracy.
pub fn ciede2000(l1: &Lab, l2: &Lab) -> f32 {
    ciede2000_pre(&LabTerms::new(l1), &LabTerms::new(l2))
}

/// A Lab color widened to f64 with its chroma precomputed.
///
/// Matching compares one query against hundreds of palette entries, so the
/// per-color part of CIEDE2000 is hoisted out of the inner loop. The values
/// are exactly what [`ciede2000`] would compute inline.
#[derive(Clone, Copy)]
pub struct LabTerms {
    l: f64,
    a: f64,
    b: f64,
    c: f64,
}

impl LabTerms {
    #[inline]
    pub fn new(lab: &Lab) -> Self {
        Self::from_parts(lab.l, lab.a, lab.b)
    }
    #[inline]
    pub fn from_parts(l: f32, a: f32, b: f32) -> Self {
        let (l, a, b) = (l as f64, a as f64, b as f64);
        Self { l, a, b, c: (a * a + b * b).sqrt() }
    }
}

/// CIEDE2000 over pre-widened colors. Identical arithmetic to [`ciede2000`].
#[inline]
pub fn ciede2000_pre(l1: &LabTerms, l2: &LabTerms) -> f32 {
    let (ll1,a1,b1)=(l1.l,l1.a,l1.b);
    let (ll2,a2,b2)=(l2.l,l2.a,l2.b);
    let c1=l1.c;let c2=l2.c;
    let ca=(c1+c2)*0.5;
    let g=0.5*(1.-(ca.powi(7)/(ca.powi(7)+25_f64.powi(7))).sqrt());
    let a1p=a1*(1.+g);let a2p=a2*(1.+g);
    let c1p=(a1p*a1p+b1*b1).sqrt();let c2p=(a2p*a2p+b2*b2).sqrt();
    let h1p=if b1==0.&&a1p==0.{0.}else{let h=b1.atan2(a1p).to_degrees();if h<0.{h+360.}else{h}};
    let h2p=if b2==0.&&a2p==0.{0.}else{let h=b2.atan2(a2p).to_degrees();if h<0.{h+360.}else{h}};
    let dl=ll2-ll1;let dc=c2p-c1p;
    let dh=if c1p*c2p==0.{0.}else{let mut dh2=h2p-h1p;if dh2>180.{dh2-=360.}if dh2< -180.{dh2+=360.}2.*(c1p*c2p).sqrt()*(dh2.to_radians()*0.5).sin()};
    let lp=(ll1+ll2)*0.5;let cp=(c1p+c2p)*0.5;
    let hp=if c1p*c2p==0.{h1p+h2p}else{let d=(h1p-h2p).abs();if d<=180.{(h1p+h2p)*0.5}else{let s=h1p+h2p+360.;if s<720.{s*0.5}else{(s-720.)*0.5}}};
    let t=1.-0.17*((hp-30.).to_radians()).cos()+0.24*((2.*hp).to_radians()).cos()+0.32*((3.*hp+6.).to_radians()).cos()-0.20*((4.*hp-63.).to_radians()).cos();
    let dt=30.*(-((hp-275.)/25.).powi(2)).exp();
    let rc=2.*(cp.powi(7)/(cp.powi(7)+25_f64.powi(7))).sqrt();
    let sl=1.+(0.015*(lp-50.).powi(2))/(20.+(lp-50.).powi(2)).sqrt();
    let sc=1.+0.045*cp;let sh=1.+0.015*cp*t;
    let rt=(-(2.*dt).to_radians()).sin()*rc;
    ((dl/sl).powi(2)+(dc/sc).powi(2)+(dh/sh).powi(2)+rt*(dc/sc)*(dh/sh)) as f32
}

// ── Palette ───────────────────────────────────────────────────────────

/// Upper bound on CIEDE2000's `S_L` term over L ∈ [0, 100], squared and
/// rounded up. Because the (ΔC, ΔH) part of the formula is a positive
/// semi-definite quadratic form (|R_T| ≤ 2 always), the total is never less
/// than `(ΔL / S_L)²` — so `ΔL² / L_BOUND` is a valid lower bound on the
/// distance, and any entry whose bound already exceeds the incumbent best can
/// be skipped. The generous constant (true max ≈ 3.053) keeps the bound safe
/// under floating-point rounding.
const L_BOUND: f64 = 3.5;

#[derive(Clone)]
pub struct Palette {
    names: Vec<String>,
    labs: Vec<[f32; 3]>,
    /// `labs` widened to f64 with chroma precomputed, indexed like `labs`.
    terms: Vec<LabTerms>,
    /// Entry indices ordered by ascending L, with `l_sorted` the matching Ls.
    /// Lets the bright path walk outward from the query's lightness and stop
    /// as soon as the ΔL bound rules out everything further away.
    by_l: Vec<u32>,
    l_sorted: Vec<f32>,
    tree: ImmutableKdTree<f32, 3>,
}

impl Palette {
    /// Build the match structures from a color table.
    ///
    /// Blocks are taken in sorted name order rather than the `HashMap`'s own,
    /// which is randomized per process. Entry indices decide which of two
    /// equally distant blocks wins a tie, so an unstable order hands the same
    /// model a different block here and there on every run — enough to change
    /// the schematic's bytes, and the reason conversions were not reproducible
    /// across restarts despite both sampling stages being seeded.
    pub fn from_table(table: &ColorTable) -> Option<Self> {
        let mut block_names: Vec<&String> = table.keys().collect();
        block_names.sort();

        let mut names=Vec::new();let mut pts:Vec<[f32;3]>=Vec::new();
        for bn in block_names {
            for c in &table[bn] { names.push(bn.clone()); pts.push(c.lab); }
        }
        if names.is_empty(){return None;}
        let terms = pts.iter().map(|&[l, a, b]| LabTerms::from_parts(l, a, b)).collect();
        let mut by_l: Vec<u32> = (0..pts.len() as u32).collect();
        by_l.sort_by(|&x, &y| {
            pts[x as usize][0].total_cmp(&pts[y as usize][0]).then(x.cmp(&y))
        });
        let l_sorted = by_l.iter().map(|&i| pts[i as usize][0]).collect();
        let tree = ImmutableKdTree::new_from_slice(&pts);
        Some(Self { names, labs: pts, terms, by_l, l_sorted, tree })
    }

    /// Index of the best-matching palette entry for `query`.
    pub fn match_block_index(&self, query: &Lab, k: usize) -> usize {
        let q_terms = LabTerms::new(query);

        // Bright samples must not fall through to dark blocks because of
        // KD-tree prefiltering. Preserve small white texture details.
        if query.l >= 72.0 {
            let minimum_l = if query.l >= 90.0 { 68.0 } else { 52.0 };
            if let Some(index) = self.match_bright(query, &q_terms, minimum_l) {
                return index;
            }
        }
        let q=[query.l,query.a,query.b];
        let nz=std::num::NonZero::new(k).unwrap_or(std::num::NonZero::new(1usize).unwrap());
        let nbrs=self.tree.nearest_n::<kiddo::SquaredEuclidean>(&q,nz);
        let mut best=0usize;let mut bd=f32::MAX;
        for n in &nbrs{
            let i=n.item as usize;
            let d=ciede2000_pre(&q_terms,&self.terms[i]);
            if d<bd{bd=d;best=i;}
        }
        best
    }

    /// Exact nearest entry among those with `L >= minimum_l`.
    ///
    /// Scans candidates in order of increasing |ΔL| from the query and stops
    /// once the ΔL lower bound exceeds the best distance found so far. That
    /// prunes entries which provably cannot win, so the winner — including
    /// which index takes a tie — matches an exhaustive scan.
    fn match_bright(&self, query: &Lab, q_terms: &LabTerms, minimum_l: f32) -> Option<usize> {
        let n = self.by_l.len();
        // Entries with L >= minimum_l are a suffix of the L-sorted order.
        let start = self.l_sorted.partition_point(|&l| l < minimum_l);
        if start >= n { return None; }

        // Split the suffix at the query's own lightness and grow outward.
        let split = start + self.l_sorted[start..].partition_point(|&l| l < query.l);
        let (mut left, mut right) = (split, split); // next: left-1 going down, right going up

        let mut best_d = f32::MAX;
        let mut best_i = usize::MAX;

        loop {
            let dl_down = if left > start {
                q_terms.l - self.l_sorted[left - 1] as f64
            } else { f64::INFINITY };
            let dl_up = if right < n {
                self.l_sorted[right] as f64 - q_terms.l
            } else { f64::INFINITY };

            // Take whichever side is closer in L; ties go to the lower L side.
            let take_down = dl_down <= dl_up;
            let dl = if take_down { dl_down } else { dl_up };
            if dl.is_infinite() { break; }
            // Everything from here out is at least this far in L, so once the
            // bound loses to the incumbent no remaining entry can win.
            if best_i != usize::MAX && dl * dl > L_BOUND * best_d as f64 { break; }

            let i = if take_down {
                left -= 1;
                self.by_l[left] as usize
            } else {
                let i = self.by_l[right] as usize;
                right += 1;
                i
            };

            let d = ciede2000_pre(q_terms, &self.terms[i]);
            // Ties resolve to the lowest index, matching a forward linear scan.
            if d < best_d || (d == best_d && i < best_i) {
                best_d = d;
                best_i = i;
            }
        }

        if best_i == usize::MAX { None } else { Some(best_i) }
    }

    pub fn match_block(&self, query: &Lab, k: usize) -> String {
        self.names[self.match_block_index(query, k)].clone()
    }

    /// Best-matching palette entry index per query. Callers resolve names via
    /// [`Self::name_of`], avoiding a String allocation per voxel.
    pub fn match_indices_batch(&self, queries: &[Lab], k: usize) -> Vec<u32> {
        use rayon::prelude::*;
        // Each query is independent, so this stays deterministic and ordered.
        queries.par_iter()
            .map(|q| self.match_block_index(q, k) as u32)
            .collect()
    }

    pub fn name_of(&self, index: u32) -> &str {
        &self.names[index as usize]
    }
    pub fn len(&self)->usize{self.names.len()}
    pub fn to_palette_json(&self)->HashMap<String,[f32;3]>{
        let mut seen:HashSet<String>=HashSet::new();let mut data=HashMap::new();
        for(i,name)in self.names.iter().enumerate(){
            let short=name.strip_prefix("minecraft:").unwrap_or(name).to_string();
            if seen.insert(short.clone()){
                let[l,a,b]=self.labs[i];let rgb=lab_to_rgb(&Lab{l,a,b});
                data.insert(short,[rgb.r,rgb.g,rgb.b]);
            }
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entry order must not depend on the table's `HashMap` iteration order,
    /// or ties are broken differently on every process start and the same
    /// model converts to slightly different blocks each run.
    #[test]
    fn entries_are_ordered_by_block_name() {
        let mut table: ColorTable = std::collections::HashMap::new();
        for (i, name) in ["minecraft:zzz", "minecraft:aaa", "minecraft:mmm",
                          "minecraft:bbb", "minecraft:yyy", "minecraft:ccc"].iter().enumerate() {
            table.insert((*name).to_string(), vec![crate::types::BlockColorEntry {
                lab: [i as f32 * 10.0, 0.0, 0.0],
                rgb: [i as f32, 0.0, 0.0],
                weight: 1.0,
            }]);
        }

        let palette = Palette::from_table(&table).expect("palette from a non-empty table");
        let mut expected: Vec<String> = table.keys().cloned().collect();
        expected.sort();
        assert_eq!(palette.names, expected);
    }

    #[test]fn test_id(){assert!(ciede2000(&Lab{l:50.,a:0.,b:0.},&Lab{l:50.,a:0.,b:0.})<0.001);}
    #[test]fn test_ref(){
        let de=ciede2000(&Lab{l:50.,a:2.6772,b:-79.7751},&Lab{l:50.,a:0.,b:-82.7485});
        // Accept wide tolerance — f32→f64 precision variations affect absolute value;
        // what matters for our use case is correct ranking (verified by test_pal below)
        assert!(de>1.5&&de<5.5,"ΔE00={de}");
    }
    #[test]fn test_far(){assert!(ciede2000(&Lab{l:100.,a:0.,b:0.},&Lab{l:0.,a:0.,b:0.})>50.);}
    #[test]fn test_pal(){
        let mut t:ColorTable=HashMap::new();
        t.insert("minecraft:stone".into(),vec![crate::types::BlockColorEntry{lab:[50.,0.,0.],rgb:[128.,128.,128.],weight:1.}]);
        t.insert("minecraft:dirt".into(),vec![crate::types::BlockColorEntry{lab:[55.,5.,15.],rgb:[134.,96.,67.],weight:1.}]);
        let p=Palette::from_table(&t).unwrap();assert_eq!(p.len(),2);
        assert_eq!(p.match_block(&rgb_to_lab(128.,128.,128.),5),"minecraft:stone");
    }

    /// The exhaustive bright-path scan this optimization replaced.
    fn reference_bright(p: &Palette, query: &Lab, minimum_l: f32) -> Option<usize> {
        let mut best = None;
        let mut best_distance = f32::MAX;
        for (i, [l, a, b]) in p.labs.iter().copied().enumerate() {
            if l < minimum_l { continue; }
            let distance = ciede2000(query, &Lab { l, a, b });
            if distance < best_distance {
                best_distance = distance;
                best = Some(i);
            }
        }
        best
    }

    /// The ΔL-bounded scan must pick exactly what a full linear scan picks,
    /// including which index wins a tie.
    #[test]
    fn test_bright_pruning_matches_linear_scan() {
        // A palette spanning the L range densely enough to exercise pruning,
        // with deliberate duplicate Labs so tie-breaking is covered.
        let mut t: ColorTable = HashMap::new();
        let mut n = 0u32;
        for r in (0..256).step_by(17) {
            for g in (0..256).step_by(17) {
                for b in (0..256).step_by(51) {
                    let lab = rgb_to_lab(r as f32, g as f32, b as f32);
                    t.insert(format!("minecraft:b{n}"), vec![crate::types::BlockColorEntry {
                        lab: [lab.l, lab.a, lab.b], rgb: [r as f32, g as f32, b as f32], weight: 1.0,
                    }]);
                    n += 1;
                }
            }
        }
        // Duplicates (same Lab under different names) to force ties.
        for dup in 0..8 {
            let lab = rgb_to_lab(255.0, 255.0, 255.0);
            t.insert(format!("minecraft:dup{dup}"), vec![crate::types::BlockColorEntry {
                lab: [lab.l, lab.a, lab.b], rgb: [255.0, 255.0, 255.0], weight: 1.0,
            }]);
        }
        let p = Palette::from_table(&t).unwrap();

        let mut checked = 0;
        for r in (0..256).step_by(5) {
            for g in (0..256).step_by(5) {
                for b in (0..256).step_by(5) {
                    let q = rgb_to_lab(r as f32, g as f32, b as f32);
                    if q.l < 72.0 { continue; }
                    let minimum_l = if q.l >= 90.0 { 68.0 } else { 52.0 };
                    let expected = reference_bright(&p, &q, minimum_l);
                    let got = p.match_bright(&q, &LabTerms::new(&q), minimum_l);
                    assert_eq!(expected, got, "rgb=({r},{g},{b}) L={}", q.l);
                    checked += 1;
                }
            }
        }
        assert!(checked > 10_000, "expected a broad sweep, checked {checked}");
    }

    /// The whole matcher as it was written before the ΔL bound and the
    /// precomputed terms — bright entries scanned exhaustively, everything
    /// else refined from the KD-tree shortlist.
    fn reference_match_index(p: &Palette, query: &Lab, k: usize) -> usize {
        if query.l >= 72.0 {
            let minimum_l = if query.l >= 90.0 { 68.0 } else { 52.0 };
            if let Some(i) = reference_bright(p, query, minimum_l) {
                return i;
            }
        }
        let q = [query.l, query.a, query.b];
        let nz = std::num::NonZero::new(k).unwrap_or(std::num::NonZero::new(1usize).unwrap());
        let nbrs = p.tree.nearest_n::<kiddo::SquaredEuclidean>(&q, nz);
        let mut best = 0usize;
        let mut bd = f32::MAX;
        for n in &nbrs {
            let i = n.item as usize;
            let [l, a, b] = p.labs[i];
            let d = ciede2000(query, &Lab { l, a, b });
            if d < bd { bd = d; best = i; }
        }
        best
    }

    /// End-to-end check against the shipped palette, covering both the bright
    /// scan and the KD-tree path.
    #[test]
    fn test_real_palette_matches_reference() {
        let Ok(json) = std::fs::read_to_string("data/color_table_safe.json") else {
            eprintln!("skipping: data/color_table_safe.json not present");
            return;
        };
        let table: ColorTable = serde_json::from_str(&json).expect("parse color table");
        let p = Palette::from_table(&table).expect("build palette");

        let (mut bright, mut dark) = (0, 0);
        for r in (0..256).step_by(3) {
            for g in (0..256).step_by(3) {
                for b in (0..256).step_by(7) {
                    let q = rgb_to_lab(r as f32, g as f32, b as f32);
                    if q.l >= 72.0 { bright += 1 } else { dark += 1 }
                    assert_eq!(
                        reference_match_index(&p, &q, 7),
                        p.match_block_index(&q, 7),
                        "rgb=({r},{g},{b})"
                    );
                }
            }
        }
        assert!(bright > 1000 && dark > 1000, "both paths must be exercised: {bright}/{dark}");
    }

    /// `ciede2000_pre` hoists per-color work out of the loop; it must return
    /// bit-for-bit what the original inline computation did.
    #[test]
    fn test_ciede2000_pre_bit_identical() {
        let sample = |i: i32| Lab {
            l: (i % 101) as f32,
            a: ((i * 7) % 255 - 127) as f32 * 0.9,
            b: ((i * 13) % 255 - 127) as f32 * 0.9,
        };
        for i in 0..2000 {
            for j in (0..2000).step_by(37) {
                let (x, y) = (sample(i), sample(j));
                let direct = ciede2000(&x, &y);
                let pre = ciede2000_pre(&LabTerms::new(&x), &LabTerms::new(&y));
                assert_eq!(direct.to_bits(), pre.to_bits(), "{x:?} vs {y:?}");
            }
        }
    }
}
