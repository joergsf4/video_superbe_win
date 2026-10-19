//! Hardware-Decodierung von H.265/HEVC über die Direct3D-11-Video-API (DXVA) der Grafikkarte.
//!
//! Windows liefert Programmen ohne Installation keinen HEVC-Decoder, die Grafikkarte kann HEVC aber in Hardware
//! (Intel Quick Sync, NVIDIA, AMD). Hier werden Parametersätze und Slice-Köpfe selbst gelesen, Bildfolge und
//! Referenzbilder (POC, RPS, DPB) verwaltet und die Slices im „short“-Format an den Treiber übergeben.
//! Unterstützt: Main und Main10 (4:2:0, 8/10 Bit) ohne Skalierungslisten. Alles andere nimmt der Software-Decoder.

use std::mem::size_of;
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010};

// ---------------------------------------------------------------------------------------------
// Bitleser
// ---------------------------------------------------------------------------------------------

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8]) -> Self {
        Bits { d, pos: 0 }
    }
    fn u(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *self.d.get(self.pos >> 3)?;
            v = (v << 1) | ((byte >> (7 - (self.pos & 7))) & 1) as u32;
            self.pos += 1;
        }
        Some(v)
    }
    fn flag(&mut self) -> Option<bool> {
        self.u(1).map(|v| v != 0)
    }
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.u(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some(((1u64 << zeros) - 1 + self.u(zeros)? as u64) as u32)
    }
    fn se(&mut self) -> Option<i32> {
        let k = self.ue()? as i64;
        Some(if k & 1 == 1 { (k + 1) / 2 } else { -(k / 2) } as i32)
    }
    fn skip(&mut self, n: usize) {
        self.pos += n;
    }
}

/// NAL-Nutzdaten ohne Emulation-Prevention-Bytes (00 00 03 → 00 00).
fn unescape(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &b in nal {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
    out
}

fn ceil_log2(v: u32) -> u32 {
    if v <= 1 { 0 } else { 32 - (v - 1).leading_zeros() }
}

// ---------------------------------------------------------------------------------------------
// Parametersätze
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct Rps {
    /// (Delta-POC, von diesem Bild benutzt) – S0: negative Deltas, S1: positive
    neg: Vec<(i32, bool)>,
    pos: Vec<(i32, bool)>,
}

#[derive(Clone, Debug, Default)]
pub struct Sps {
    id: u32,
    profile_idc: u32,
    chroma_format_idc: u32,
    pub width: u32,
    pub height: u32,
    crop: [u32; 4], // links, rechts, oben, unten in Luma-Pixeln
    bit_depth_luma: u32,
    bit_depth_chroma: u32,
    log2_max_poc_lsb: u32,
    max_dec_pic_buffering_minus1: u32,
    max_num_reorder: u32,
    log2_min_cb_minus3: u32,
    log2_diff_max_min_cb: u32,
    log2_min_tb_minus2: u32,
    log2_diff_max_min_tb: u32,
    max_th_inter: u32,
    max_th_intra: u32,
    scaling_list: bool,
    amp: bool,
    sao: bool,
    pcm: bool,
    pcm_bits_luma_minus1: u32,
    pcm_bits_chroma_minus1: u32,
    log2_min_pcm_minus3: u32,
    log2_diff_pcm: u32,
    pcm_loop_filter_disabled: bool,
    st_rps: Vec<Rps>,
    lt_present: bool,
    lt_sps: Vec<(u32, bool)>,
    temporal_mvp: bool,
    strong_intra: bool,
    full_range: bool,
    matrix: u8,
    pub transfer: u8,
}

impl Sps {
    fn ctb_log2(&self) -> u32 {
        self.log2_min_cb_minus3 + 3 + self.log2_diff_max_min_cb
    }
    fn min_cb(&self) -> u32 {
        1 << (self.log2_min_cb_minus3 + 3)
    }
    fn pic_size_in_ctbs(&self) -> u32 {
        let c = 1u32 << self.ctb_log2();
        self.width.div_ceil(c) * self.height.div_ceil(c)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pps {
    id: u32,
    sps_id: u32,
    dependent_slices: bool,
    output_flag_present: bool,
    extra_slice_header_bits: u32,
    sign_data_hiding: bool,
    cabac_init_present: bool,
    l0_default_minus1: u32,
    l1_default_minus1: u32,
    init_qp_minus26: i32,
    constrained_intra: bool,
    transform_skip: bool,
    cu_qp_delta: bool,
    diff_cu_qp_delta_depth: u32,
    cb_qp_offset: i32,
    cr_qp_offset: i32,
    slice_chroma_qp_offsets: bool,
    weighted_pred: bool,
    weighted_bipred: bool,
    transquant_bypass: bool,
    tiles: bool,
    entropy_sync: bool,
    tile_cols_minus1: u32,
    tile_rows_minus1: u32,
    uniform_spacing: bool,
    col_widths_minus1: Vec<u32>,
    row_heights_minus1: Vec<u32>,
    loop_filter_across_tiles: bool,
    loop_filter_across_slices: bool,
    deblocking_override_enabled: bool,
    deblocking_disabled: bool,
    beta_offset_div2: i32,
    tc_offset_div2: i32,
    scaling_list: bool,
    lists_modification: bool,
    log2_parallel_merge_minus2: u32,
    slice_header_extension: bool,
}

fn parse_profile_tier_level(b: &mut Bits, max_sub_layers_minus1: u32) -> Option<u32> {
    b.skip(3); // profile_space, tier
    let profile_idc = b.u(5)?;
    b.skip(32 + 4 + 43 + 1); // Kompatibilität, Flags, reserviert
    b.skip(8); // general_level_idc
    let mut prof = vec![];
    let mut lvl = vec![];
    for _ in 0..max_sub_layers_minus1 {
        prof.push(b.flag()?);
        lvl.push(b.flag()?);
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            b.skip(2);
        }
    }
    for i in 0..max_sub_layers_minus1 as usize {
        if prof[i] {
            b.skip(88);
        }
        if lvl[i] {
            b.skip(8);
        }
    }
    Some(profile_idc)
}

fn parse_st_rps(b: &mut Bits, idx: usize, num_sets: usize, sets: &[Rps]) -> Option<Rps> {
    let inter = if idx != 0 { b.flag()? } else { false };
    if inter {
        let delta_idx_minus1 = if idx == num_sets { b.ue()? } else { 0 };
        let sign = b.flag()?;
        let abs_minus1 = b.ue()?;
        let ref_idx = idx.checked_sub(delta_idx_minus1 as usize + 1)?;
        let r = sets.get(ref_idx)?;
        let delta_rps = (1 - 2 * sign as i32) * (abs_minus1 as i32 + 1);
        let n_ref = r.neg.len() + r.pos.len();
        let mut used = vec![false; n_ref + 1];
        let mut use_delta = vec![true; n_ref + 1];
        for j in 0..=n_ref {
            used[j] = b.flag()?;
            if !used[j] {
                use_delta[j] = b.flag()?;
            }
        }
        let (nn, np) = (r.neg.len(), r.pos.len());
        let mut s0 = vec![];
        for j in (0..np).rev() {
            let d = r.pos[j].0 + delta_rps;
            if d < 0 && use_delta[nn + j] {
                s0.push((d, used[nn + j]));
            }
        }
        if delta_rps < 0 && use_delta[n_ref] {
            s0.push((delta_rps, used[n_ref]));
        }
        for j in 0..nn {
            let d = r.neg[j].0 + delta_rps;
            if d < 0 && use_delta[j] {
                s0.push((d, used[j]));
            }
        }
        let mut s1 = vec![];
        for j in (0..nn).rev() {
            let d = r.neg[j].0 + delta_rps;
            if d > 0 && use_delta[j] {
                s1.push((d, used[j]));
            }
        }
        if delta_rps > 0 && use_delta[n_ref] {
            s1.push((delta_rps, used[n_ref]));
        }
        for j in 0..np {
            let d = r.pos[j].0 + delta_rps;
            if d > 0 && use_delta[nn + j] {
                s1.push((d, used[nn + j]));
            }
        }
        Some(Rps { neg: s0, pos: s1 })
    } else {
        let nneg = b.ue()?;
        let npos = b.ue()?;
        if nneg > 16 || npos > 16 {
            return None;
        }
        let mut rps = Rps::default();
        let mut p = 0;
        for _ in 0..nneg {
            p -= b.ue()? as i32 + 1;
            rps.neg.push((p, b.flag()?));
        }
        p = 0;
        for _ in 0..npos {
            p += b.ue()? as i32 + 1;
            rps.pos.push((p, b.flag()?));
        }
        Some(rps)
    }
}

/// SPS aus einer NAL (mit 2-Byte-Kopf). `None`, wenn Aufbau oder Profil nicht unterstützt werden.
pub fn parse_sps(nal: &[u8]) -> Option<Sps> {
    let s = parse_sps_raw(nal)?;
    let ok = matches!(s.profile_idc, 1 | 2) && s.chroma_format_idc == 1 && s.bit_depth_luma == s.bit_depth_chroma && matches!(s.bit_depth_luma, 8 | 10);
    ok.then_some(s)
}

/// Wie `parse_sps`, aber ohne Prüfung des Profils (für Diagnose).
fn parse_sps_raw(nal: &[u8]) -> Option<Sps> {
    let rb = unescape(nal.get(2..)?);
    let mut b = Bits::new(&rb);
    let mut s = Sps::default();
    b.skip(4); // vps id
    let max_sub_layers_minus1 = b.u(3)?;
    b.skip(1);
    s.profile_idc = parse_profile_tier_level(&mut b, max_sub_layers_minus1)?;
    s.id = b.ue()?;
    s.chroma_format_idc = b.ue()?;
    if s.chroma_format_idc == 3 && b.flag()? {
        return None; // getrennte Farbebenen
    }
    s.width = b.ue()?;
    s.height = b.ue()?;
    if b.flag()? {
        let (l, r, t, bt) = (b.ue()?, b.ue()?, b.ue()?, b.ue()?);
        let (sx, sy) = if s.chroma_format_idc == 1 { (2, 2) } else if s.chroma_format_idc == 2 { (2, 1) } else { (1, 1) };
        s.crop = [l * sx, r * sx, t * sy, bt * sy];
    }
    s.bit_depth_luma = b.ue()? + 8;
    s.bit_depth_chroma = b.ue()? + 8;
    s.log2_max_poc_lsb = b.ue()? + 4;
    let sub_layer_ordering = b.flag()?;
    let start = if sub_layer_ordering { 0 } else { max_sub_layers_minus1 };
    for _ in start..=max_sub_layers_minus1 {
        s.max_dec_pic_buffering_minus1 = b.ue()?;
        s.max_num_reorder = b.ue()?;
        b.ue()?; // max_latency_increase_plus1
    }
    s.log2_min_cb_minus3 = b.ue()?;
    s.log2_diff_max_min_cb = b.ue()?;
    s.log2_min_tb_minus2 = b.ue()?;
    s.log2_diff_max_min_tb = b.ue()?;
    s.max_th_inter = b.ue()?;
    s.max_th_intra = b.ue()?;
    s.scaling_list = b.flag()?;
    if s.scaling_list && b.flag()? {
        return None; // eigene Skalierungslisten im SPS: nicht unterstützt
    }
    s.amp = b.flag()?;
    s.sao = b.flag()?;
    s.pcm = b.flag()?;
    if s.pcm {
        s.pcm_bits_luma_minus1 = b.u(4)?;
        s.pcm_bits_chroma_minus1 = b.u(4)?;
        s.log2_min_pcm_minus3 = b.ue()?;
        s.log2_diff_pcm = b.ue()?;
        s.pcm_loop_filter_disabled = b.flag()?;
    }
    let n = b.ue()? as usize;
    if n > 64 {
        return None;
    }
    for i in 0..n {
        let r = parse_st_rps(&mut b, i, n, &s.st_rps)?;
        s.st_rps.push(r);
    }
    s.lt_present = b.flag()?;
    if s.lt_present {
        let n = b.ue()?;
        if n > 32 {
            return None;
        }
        for _ in 0..n {
            let lsb = b.u(s.log2_max_poc_lsb)?;
            s.lt_sps.push((lsb, b.flag()?));
        }
    }
    s.temporal_mvp = b.flag()?;
    s.strong_intra = b.flag()?;
    // Anfang der VUI: Wertebereich und Farbmatrix (Fehler hier sind unkritisch)
    let _ = (|| -> Option<()> {
        if !b.flag()? {
            return Some(());
        }
        if b.flag()? && b.u(8)? == 255 {
            b.skip(32);
        }
        if b.flag()? {
            b.skip(1);
        }
        if b.flag()? {
            b.skip(3);
            s.full_range = b.flag()?;
            if b.flag()? {
                b.skip(8);
                s.transfer = b.u(8)? as u8;
                s.matrix = b.u(8)? as u8;
            }
        }
        Some(())
    })();
    Some(s)
}

pub fn parse_pps(nal: &[u8]) -> Option<Pps> {
    let rb = unescape(nal.get(2..)?);
    let mut b = Bits::new(&rb);
    let mut p = Pps { id: b.ue()?, sps_id: b.ue()?, ..Default::default() };
    p.dependent_slices = b.flag()?;
    p.output_flag_present = b.flag()?;
    p.extra_slice_header_bits = b.u(3)?;
    p.sign_data_hiding = b.flag()?;
    p.cabac_init_present = b.flag()?;
    p.l0_default_minus1 = b.ue()?;
    p.l1_default_minus1 = b.ue()?;
    p.init_qp_minus26 = b.se()?;
    p.constrained_intra = b.flag()?;
    p.transform_skip = b.flag()?;
    p.cu_qp_delta = b.flag()?;
    if p.cu_qp_delta {
        p.diff_cu_qp_delta_depth = b.ue()?;
    }
    p.cb_qp_offset = b.se()?;
    p.cr_qp_offset = b.se()?;
    p.slice_chroma_qp_offsets = b.flag()?;
    p.weighted_pred = b.flag()?;
    p.weighted_bipred = b.flag()?;
    p.transquant_bypass = b.flag()?;
    p.tiles = b.flag()?;
    p.entropy_sync = b.flag()?;
    if p.tiles {
        p.tile_cols_minus1 = b.ue()?;
        p.tile_rows_minus1 = b.ue()?;
        if p.tile_cols_minus1 > 18 || p.tile_rows_minus1 > 20 {
            return None;
        }
        p.uniform_spacing = b.flag()?;
        if !p.uniform_spacing {
            for _ in 0..p.tile_cols_minus1 {
                p.col_widths_minus1.push(b.ue()?);
            }
            for _ in 0..p.tile_rows_minus1 {
                p.row_heights_minus1.push(b.ue()?);
            }
        }
        p.loop_filter_across_tiles = b.flag()?;
    }
    p.loop_filter_across_slices = b.flag()?;
    if b.flag()? {
        p.deblocking_override_enabled = b.flag()?;
        p.deblocking_disabled = b.flag()?;
        if !p.deblocking_disabled {
            p.beta_offset_div2 = b.se()?;
            p.tc_offset_div2 = b.se()?;
        }
    }
    p.scaling_list = b.flag()?;
    if p.scaling_list {
        return None;
    }
    p.lists_modification = b.flag()?;
    p.log2_parallel_merge_minus2 = b.ue()?;
    p.slice_header_extension = b.flag()?;
    Some(p)
}

#[derive(Debug, Default, Clone)]
struct SliceInfo {
    first_slice: bool,
    pps_id: u32,
    poc_lsb: u32,
    rps: Rps,
    st_rps_bits: u32,
    num_delta_pocs_ref_rps: u32,
    /// Langzeitreferenzen: (POC bzw. POC-LSB, von diesem Bild benutzt, MSB vorhanden)
    lt: Vec<(i32, bool, bool)>,
}

fn parse_slice_header(nal_type: u32, rb: &[u8], sps_map: &[Option<Sps>], pps_map: &[Option<Pps>]) -> Option<SliceInfo> {
    let mut b = Bits::new(rb);
    let mut s = SliceInfo { first_slice: b.flag()?, ..Default::default() };
    if (16..=23).contains(&nal_type) {
        b.skip(1); // no_output_of_prior_pics_flag
    }
    s.pps_id = b.ue()?;
    let pps = pps_map.get(s.pps_id as usize)?.as_ref()?;
    let sps = sps_map.get(pps.sps_id as usize)?.as_ref()?;
    if !s.first_slice {
        return Some(s);
    }
    // (nur der erste Slice eines Bildes bestimmt die Bildparameter)
    let _ = pps;
    if nal_type != 19 && nal_type != 20 {
        // vor slice_pic_order_cnt_lsb stehen noch: slice_type, pic_output_flag
    }
    for _ in 0..pps.extra_slice_header_bits {
        b.skip(1);
    }
    let _slice_type = b.ue()?;
    if pps.output_flag_present {
        b.skip(1);
    }
    if nal_type != 19 && nal_type != 20 {
        s.poc_lsb = b.u(sps.log2_max_poc_lsb)?;
        let from_sps = b.flag()?;
        if !from_sps {
            let start = b.pos;
            s.rps = parse_st_rps(&mut b, sps.st_rps.len(), sps.st_rps.len(), &sps.st_rps)?;
            s.st_rps_bits = (b.pos - start) as u32;
        } else {
            let idx = if sps.st_rps.len() > 1 { b.u(ceil_log2(sps.st_rps.len() as u32))? } else { 0 };
            s.rps = sps.st_rps.get(idx as usize)?.clone();
        }
        if sps.lt_present {
            let num_lt_sps = if !sps.lt_sps.is_empty() { b.ue()? } else { 0 };
            let num_lt_pics = b.ue()?;
            let mut prev_msb = 0i64;
            for i in 0..(num_lt_sps + num_lt_pics) {
                let (lsb, used) = if i < num_lt_sps {
                    let idx = if sps.lt_sps.len() > 1 { b.u(ceil_log2(sps.lt_sps.len() as u32))? } else { 0 };
                    *sps.lt_sps.get(idx as usize)?
                } else {
                    let lsb = b.u(sps.log2_max_poc_lsb)?;
                    (lsb, b.flag()?)
                };
                let msb_present = b.flag()?;
                let mut cycle = 0i64;
                if msb_present {
                    cycle = b.ue()? as i64;
                }
                if i != 0 && i != num_lt_sps {
                    cycle += prev_msb;
                }
                prev_msb = cycle;
                // Wert: LSB; bei vorhandenem MSB wird später relativ zum aktuellen POC umgerechnet
                let v = if msb_present { -(cycle as i32 * (1 << sps.log2_max_poc_lsb)) + lsb as i32 } else { lsb as i32 };
                s.lt.push((v, used, msb_present));
            }
        }
    }
    s.num_delta_pocs_ref_rps = 0;
    Some(s)
}

// ---------------------------------------------------------------------------------------------
// DXVA-Strukturen
// ---------------------------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct PicParams {
    pic_width_in_min_cbs_y: u16,
    pic_height_in_min_cbs_y: u16,
    format_flags: u16,
    curr_pic: u8,
    sps_max_dec_pic_buffering_minus1: u8,
    log2_min_luma_cb_minus3: u8,
    log2_diff_max_min_luma_cb: u8,
    log2_min_tb_minus2: u8,
    log2_diff_max_min_tb: u8,
    max_th_depth_inter: u8,
    max_th_depth_intra: u8,
    num_st_rps: u8,
    num_lt_ref_pics_sps: u8,
    num_ref_idx_l0_default_minus1: u8,
    num_ref_idx_l1_default_minus1: u8,
    init_qp_minus26: i8,
    num_delta_pocs_of_ref_rps_idx: u8,
    num_bits_for_st_rps_in_slice: u16,
    reserved2: u16,
    coding_tool_flags: u32,
    coding_setting_flags: u32,
    pps_cb_qp_offset: i8,
    pps_cr_qp_offset: i8,
    num_tile_columns_minus1: u8,
    num_tile_rows_minus1: u8,
    column_width_minus1: [u16; 19],
    row_height_minus1: [u16; 21],
    diff_cu_qp_delta_depth: u8,
    pps_beta_offset_div2: i8,
    pps_tc_offset_div2: i8,
    log2_parallel_merge_level_minus2: u8,
    curr_pic_order_cnt_val: i32,
    ref_pic_list: [u8; 15],
    reserved5: u8,
    poc_list: [i32; 15],
    st_curr_before: [u8; 8],
    st_curr_after: [u8; 8],
    lt_curr: [u8; 8],
    reserved6: u16,
    reserved7: u16,
    status_report: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SliceShort {
    bs_nal_unit_data_location: u32,
    slice_bytes_in_buffer: u32,
    bad_slice_chopping: u16,
}

/// Standard-Skalierungslisten (Tabelle 7-6, in Scan-Reihenfolge) für `DXVA_Qmatrix_HEVC`, wenn der SPS
/// Skalierungslisten einschaltet, aber keine eigenen mitliefert (so macht es das iPhone).
fn default_qmatrix() -> Vec<u8> {
    const INTRA: [u8; 64] = [
        16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 17, 16, 17, 16, 17, 18, 17, 18, 18, 17, 18, 21, 19, 20, 21, 20, 19, 21, 24, 22, 22, 24, 24, 22, 22, 24, 25, 25, 27, 30, 27, 25, 25, 29, 31,
        35, 35, 31, 29, 36, 41, 44, 41, 36, 47, 54, 54, 47, 65, 70, 65, 88, 88, 115,
    ];
    const INTER: [u8; 64] = [
        16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 17, 17, 17, 17, 17, 18, 18, 18, 18, 18, 18, 20, 20, 20, 20, 20, 20, 20, 24, 24, 24, 24, 24, 24, 24, 24, 25, 25, 25, 25, 25, 25, 25, 28, 28,
        28, 28, 28, 28, 33, 33, 33, 33, 33, 41, 41, 41, 41, 54, 54, 54, 71, 71, 91,
    ];
    let mut q = vec![];
    q.extend(std::iter::repeat_n(16u8, 6 * 16)); // 4x4
    for list8 in [&INTRA, &INTRA, &INTRA, &INTER, &INTER, &INTER] {
        q.extend_from_slice(list8); // 8x8
    }
    for list8 in [&INTRA, &INTRA, &INTRA, &INTER, &INTER, &INTER] {
        q.extend_from_slice(list8); // 16x16
    }
    q.extend_from_slice(&INTRA); // 32x32 intra
    q.extend_from_slice(&INTER); // 32x32 inter
    q.extend(std::iter::repeat_n(16u8, 6 + 2)); // DC 16x16, 32x32
    q
}

// ---------------------------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct DpbPic {
    slot: usize,
    poc: i32,
    long_term: bool,
}

pub struct GpuDecoder {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    vctx: ID3D11VideoContext,
    decoder: ID3D11VideoDecoder,
    texture: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    views: Vec<ID3D11VideoDecoderOutputView>,
    format: DXGI_FORMAT,
    aligned_w: u32,
    aligned_h: u32,
    pub sps: Sps,
    sps_map: Vec<Option<Sps>>,
    pps_map: Vec<Option<Pps>>,
    nal_len: usize,
    dpb: Vec<DpbPic>,
    free: Vec<usize>,
    prev_tid0_poc: i32,
    first_after_reset: bool,
    skip_rasl: bool,
    status_counter: u32,
    pub last_slot: usize,
}

impl GpuDecoder {
    /// Legt Gerät, Decoder und Oberflächen an. `None`, wenn Grafikkarte, Profil oder Stream nicht passen.
    pub fn new(params: &[Vec<u8>], nal_len: usize) -> Option<GpuDecoder> {
        let mut sps_map: Vec<Option<Sps>> = vec![None; 16];
        let mut pps_map: Vec<Option<Pps>> = vec![None; 64];
        let mut main: Option<Sps> = None;
        for nal in params {
            match (nal.first()? >> 1) & 0x3F {
                33 => {
                    let s = parse_sps(nal)?;
                    if (s.id as usize) < 16 {
                        main.get_or_insert_with(|| s.clone());
                        let i = s.id as usize;
                        sps_map[i] = Some(s);
                    }
                }
                34 => {
                    let p = parse_pps(nal)?;
                    if (p.id as usize) < 64 {
                        let i = p.id as usize;
                        pps_map[i] = Some(p);
                    }
                }
                _ => {}
            }
        }
        let sps = main?;
        unsafe {
            let mut dev: Option<ID3D11Device> = None;
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, None).ok()?;
            let device = dev?;
            if let Ok(mt) = device.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let vdev = device.cast::<ID3D11VideoDevice>().ok()?;
            let ctx = device.GetImmediateContext().ok()?;
            let vctx = ctx.cast::<ID3D11VideoContext>().ok()?;
            let (profile, format) = if sps.bit_depth_luma == 10 { (D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10, DXGI_FORMAT_P010) } else { (D3D11_DECODER_PROFILE_HEVC_VLD_MAIN, DXGI_FORMAT_NV12) };
            if !vdev.CheckVideoDecoderFormat(&profile, format).ok()?.as_bool() {
                return None;
            }
            let aw = sps.width.div_ceil(128) * 128;
            let ah = sps.height.div_ceil(128) * 128;
            let desc = D3D11_VIDEO_DECODER_DESC { Guid: profile, SampleWidth: aw, SampleHeight: ah, OutputFormat: format };
            let n_cfg = vdev.GetVideoDecoderConfigCount(&desc).ok()?;
            let mut cfg = None;
            for i in 0..n_cfg {
                let mut c = D3D11_VIDEO_DECODER_CONFIG::default();
                if vdev.GetVideoDecoderConfig(&desc, i, &mut c).is_ok() && c.ConfigBitstreamRaw == 1 {
                    cfg = Some(c);
                    break;
                }
            }
            let cfg = cfg?;
            let decoder = vdev.CreateVideoDecoder(&desc, &cfg).ok()?;
            let pool = (sps.max_dec_pic_buffering_minus1 as usize + 1 + 4).clamp(6, 24);
            let td = D3D11_TEXTURE2D_DESC {
                Width: aw,
                Height: ah,
                MipLevels: 1,
                ArraySize: pool as u32,
                Format: format,
                SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_DECODER.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut tex: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&td, None, Some(&mut tex)).ok()?;
            let texture = tex?;
            let sd = D3D11_TEXTURE2D_DESC { ArraySize: 1, BindFlags: 0, Usage: D3D11_USAGE_STAGING, CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32, ..td };
            let mut st: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&sd, None, Some(&mut st)).ok()?;
            let staging = st?;
            let mut views = vec![];
            for i in 0..pool {
                let vd = D3D11_VIDEO_DECODER_OUTPUT_VIEW_DESC {
                    DecodeProfile: profile,
                    ViewDimension: D3D11_VDOV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_DECODER_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VDOV { ArraySlice: i as u32 } },
                };
                let mut v: Option<ID3D11VideoDecoderOutputView> = None;
                vdev.CreateVideoDecoderOutputView(&texture, &vd, Some(&mut v)).ok()?;
                views.push(v?);
            }
            Some(GpuDecoder {
                device,
                ctx,
                vctx,
                decoder,
                texture,
                staging,
                views,
                format,
                aligned_w: aw,
                aligned_h: ah,
                sps,
                sps_map,
                pps_map,
                nal_len,
                dpb: vec![],
                free: (0..pool).rev().collect(),
                prev_tid0_poc: 0,
                first_after_reset: true,
                skip_rasl: false,
                status_counter: 0,
                last_slot: 0,
            })
        }
    }

    /// Vor einem Sprung: Referenzbilder verwerfen.
    pub fn reset(&mut self) {
        for p in self.dpb.drain(..) {
            self.free.push(p.slot);
        }
        self.first_after_reset = true;
        self.skip_rasl = false;
        self.prev_tid0_poc = 0;
    }

    /// Decodiert ein MP4-Sample (längenpräfixierte NALs). `Ok(Some(slot))`: Bild fertig, `Ok(None)`: übersprungen.
    pub fn decode_sample(&mut self, data: &[u8]) -> Result<Option<usize>, String> {
        let nl = self.nal_len;
        let mut p = 0usize;
        let mut slices: Vec<&[u8]> = vec![];
        while p + nl <= data.len() {
            let len = data[p..p + nl].iter().fold(0usize, |a, &b| (a << 8) | b as usize);
            p += nl;
            if len < 2 || p + len > data.len() {
                break;
            }
            let nal = &data[p..p + len];
            p += len;
            let t = ((nal[0] >> 1) & 0x3F) as u32;
            match t {
                33 => {
                    if let Some(s) = parse_sps(nal) {
                        let i = s.id as usize;
                        if i < 16 {
                            self.sps_map[i] = Some(s);
                        }
                    }
                }
                34 => {
                    if let Some(pp) = parse_pps(nal) {
                        let i = pp.id as usize;
                        if i < 64 {
                            self.pps_map[i] = Some(pp);
                        }
                    }
                }
                0..=9 | 16..=21 => slices.push(nal),
                _ => {}
            }
        }
        let Some(first) = slices.first() else { return Ok(None) };
        let nal_type = ((first[0] >> 1) & 0x3F) as u32;
        let tid = (first[1] & 7) as i32 - 1;
        let rb = unescape(&first[2..]);
        let sh = parse_slice_header(nal_type, &rb, &self.sps_map, &self.pps_map).ok_or("Slice-Kopf nicht lesbar")?;
        if !sh.first_slice {
            return Err("Bild beginnt nicht mit dem ersten Slice".into());
        }
        let pps = self.pps_map[sh.pps_id as usize].clone().ok_or("PPS fehlt")?;
        let sps = self.sps_map[pps.sps_id as usize].clone().ok_or("SPS fehlt")?;
        if sps.width != self.sps.width || sps.height != self.sps.height || sps.bit_depth_luma != self.sps.bit_depth_luma {
            return Err("Bildformat wechselt mitten im Video".into());
        }
        let irap = (16..=23).contains(&nal_type);
        let idr = nal_type == 19 || nal_type == 20;
        let rasl = nal_type == 8 || nal_type == 9;
        let radl = nal_type == 6 || nal_type == 7;
        let sub_layer_non_ref = nal_type < 16 && nal_type % 2 == 0;
        let no_rasl_output = irap && (matches!(nal_type, 16..=20) || self.first_after_reset);
        if irap {
            self.skip_rasl = no_rasl_output;
        }
        if rasl && self.skip_rasl {
            return Ok(None);
        }
        if self.first_after_reset && !irap {
            return Err("Sprung nicht auf ein Schlüsselbild".into());
        }
        // POC (8.3.1)
        let max_lsb = 1i32 << sps.log2_max_poc_lsb;
        let lsb = sh.poc_lsb as i32;
        let msb = if irap && no_rasl_output {
            0
        } else {
            let prev_lsb = self.prev_tid0_poc & (max_lsb - 1);
            let prev_msb = self.prev_tid0_poc - prev_lsb;
            if lsb < prev_lsb && prev_lsb - lsb >= max_lsb / 2 {
                prev_msb + max_lsb
            } else if lsb > prev_lsb && lsb - prev_lsb > max_lsb / 2 {
                prev_msb - max_lsb
            } else {
                prev_msb
            }
        };
        let poc = msb + lsb;
        if tid == 0 && !rasl && !radl && !sub_layer_non_ref {
            self.prev_tid0_poc = poc;
        }
        self.first_after_reset = false;
        if irap && no_rasl_output {
            for pic in self.dpb.drain(..) {
                self.free.push(pic.slot);
            }
        }
        // Referenzbildmenge (8.3.2)
        let find_st = |dpb: &[DpbPic], target: i32| dpb.iter().position(|d| !d.long_term && d.poc == target);
        let mut before: Vec<usize> = vec![]; // Positionen im DPB
        let mut after: Vec<usize> = vec![];
        let mut lt_curr: Vec<usize> = vec![];
        let mut keep = vec![false; self.dpb.len()];
        let mut lt_mark: Vec<usize> = vec![];
        for &(d, used) in &sh.rps.neg {
            if let Some(i) = find_st(&self.dpb, poc + d) {
                keep[i] = true;
                if used {
                    before.push(i);
                }
            }
        }
        for &(d, used) in &sh.rps.pos {
            if let Some(i) = find_st(&self.dpb, poc + d) {
                keep[i] = true;
                if used {
                    after.push(i);
                }
            }
        }
        for &(v, used, msb_present) in &sh.lt {
            let i = if msb_present {
                let target = poc - (poc & (max_lsb - 1)) + v; // v = LSB − Zyklus·MaxLsb
                self.dpb.iter().position(|d| d.poc == target)
            } else {
                self.dpb.iter().position(|d| (d.poc & (max_lsb - 1)) == v)
            };
            if let Some(i) = i {
                keep[i] = true;
                lt_mark.push(i);
                if used {
                    lt_curr.push(i);
                }
            }
        }
        for i in lt_mark {
            self.dpb[i].long_term = true;
        }
        // nicht mehr referenzierte Bilder freigeben (von hinten, damit Positionen gültig bleiben)
        let mut remap: Vec<Option<usize>> = vec![None; self.dpb.len()];
        let mut new_dpb = vec![];
        for (i, pic) in self.dpb.iter().enumerate() {
            if keep[i] {
                remap[i] = Some(new_dpb.len());
                new_dpb.push(*pic);
            } else {
                self.free.push(pic.slot);
            }
        }
        self.dpb = new_dpb;
        let fix = |v: Vec<usize>| -> Vec<usize> { v.into_iter().filter_map(|i| remap[i]).collect() };
        let (before, after, lt_curr) = (fix(before), fix(after), fix(lt_curr));

        let slot = self.free.pop().ok_or("keine freie Oberfläche")?;
        let result = self.submit(&sps, &pps, &sh, nal_type, poc, slot, &slices, (&before, &after, &lt_curr));
        match result {
            Ok(()) => {
                self.dpb.push(DpbPic { slot, poc, long_term: false });
                self.last_slot = slot;
                Ok(Some(slot))
            }
            Err(e) => {
                self.free.push(slot);
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn submit(&mut self, sps: &Sps, pps: &Pps, sh: &SliceInfo, nal_type: u32, poc: i32, slot: usize, slices: &[&[u8]], refs: (&[usize], &[usize], &[usize])) -> Result<(), String> {
        let irap = (16..=23).contains(&nal_type);
        let idr = nal_type == 19 || nal_type == 20;
        let mut pp: PicParams = unsafe { std::mem::zeroed() };
        pp.pic_width_in_min_cbs_y = (sps.width / sps.min_cb()) as u16;
        pp.pic_height_in_min_cbs_y = (sps.height / sps.min_cb()) as u16;
        pp.format_flags = (sps.chroma_format_idc as u16 & 3)
            | (((sps.bit_depth_luma - 8) as u16 & 7) << 3)
            | (((sps.bit_depth_chroma - 8) as u16 & 7) << 6)
            | (((sps.log2_max_poc_lsb - 4) as u16 & 15) << 9)
            | (((sps.max_num_reorder == 0) as u16) << 13);
        pp.curr_pic = slot as u8;
        pp.sps_max_dec_pic_buffering_minus1 = sps.max_dec_pic_buffering_minus1 as u8;
        pp.log2_min_luma_cb_minus3 = sps.log2_min_cb_minus3 as u8;
        pp.log2_diff_max_min_luma_cb = sps.log2_diff_max_min_cb as u8;
        pp.log2_min_tb_minus2 = sps.log2_min_tb_minus2 as u8;
        pp.log2_diff_max_min_tb = sps.log2_diff_max_min_tb as u8;
        pp.max_th_depth_inter = sps.max_th_inter as u8;
        pp.max_th_depth_intra = sps.max_th_intra as u8;
        pp.num_st_rps = sps.st_rps.len() as u8;
        pp.num_lt_ref_pics_sps = sps.lt_sps.len() as u8;
        pp.num_ref_idx_l0_default_minus1 = pps.l0_default_minus1 as u8;
        pp.num_ref_idx_l1_default_minus1 = pps.l1_default_minus1 as u8;
        pp.init_qp_minus26 = pps.init_qp_minus26 as i8;
        pp.num_delta_pocs_of_ref_rps_idx = sh.num_delta_pocs_ref_rps as u8;
        pp.num_bits_for_st_rps_in_slice = sh.st_rps_bits as u16;
        let b = |v: bool, bit: u32| (v as u32) << bit;
        pp.coding_tool_flags = b(sps.scaling_list, 0)
            | b(sps.amp, 1)
            | b(sps.sao, 2)
            | b(sps.pcm, 3)
            | ((sps.pcm_bits_luma_minus1 & 15) << 4)
            | ((sps.pcm_bits_chroma_minus1 & 15) << 8)
            | ((sps.log2_min_pcm_minus3 & 3) << 12)
            | ((sps.log2_diff_pcm & 3) << 14)
            | b(sps.pcm_loop_filter_disabled, 16)
            | b(sps.lt_present, 17)
            | b(sps.temporal_mvp, 18)
            | b(sps.strong_intra, 19)
            | b(pps.dependent_slices, 20)
            | b(pps.output_flag_present, 21)
            | ((pps.extra_slice_header_bits & 7) << 22)
            | b(pps.sign_data_hiding, 25)
            | b(pps.cabac_init_present, 26);
        pp.coding_setting_flags = b(pps.constrained_intra, 0)
            | b(pps.transform_skip, 1)
            | b(pps.cu_qp_delta, 2)
            | b(pps.slice_chroma_qp_offsets, 3)
            | b(pps.weighted_pred, 4)
            | b(pps.weighted_bipred, 5)
            | b(pps.transquant_bypass, 6)
            | b(pps.tiles, 7)
            | b(pps.entropy_sync, 8)
            | b(pps.uniform_spacing, 9)
            | b(pps.loop_filter_across_tiles, 10)
            | b(pps.loop_filter_across_slices, 11)
            | b(pps.deblocking_override_enabled, 12)
            | b(pps.deblocking_disabled, 13)
            | b(pps.lists_modification, 14)
            | b(pps.slice_header_extension, 15)
            | b(irap, 16)
            | b(idr, 17)
            | b(irap, 18);
        pp.pps_cb_qp_offset = pps.cb_qp_offset as i8;
        pp.pps_cr_qp_offset = pps.cr_qp_offset as i8;
        if pps.tiles {
            pp.num_tile_columns_minus1 = pps.tile_cols_minus1 as u8;
            pp.num_tile_rows_minus1 = pps.tile_rows_minus1 as u8;
            let ctb = 1u32 << sps.ctb_log2();
            let (wc, hc) = (sps.width.div_ceil(ctb), sps.height.div_ceil(ctb));
            let (cols, rows) = (pps.tile_cols_minus1 + 1, pps.tile_rows_minus1 + 1);
            if pps.uniform_spacing {
                for i in 0..pps.tile_cols_minus1 {
                    pp.column_width_minus1[i as usize] = (((i + 1) * wc) / cols - (i * wc) / cols - 1) as u16;
                }
                for i in 0..pps.tile_rows_minus1 {
                    pp.row_height_minus1[i as usize] = (((i + 1) * hc) / rows - (i * hc) / rows - 1) as u16;
                }
            } else {
                for (i, v) in pps.col_widths_minus1.iter().enumerate() {
                    pp.column_width_minus1[i] = *v as u16;
                }
                for (i, v) in pps.row_heights_minus1.iter().enumerate() {
                    pp.row_height_minus1[i] = *v as u16;
                }
            }
        }
        pp.diff_cu_qp_delta_depth = pps.diff_cu_qp_delta_depth as u8;
        pp.pps_beta_offset_div2 = pps.beta_offset_div2 as i8;
        pp.pps_tc_offset_div2 = pps.tc_offset_div2 as i8;
        pp.log2_parallel_merge_level_minus2 = pps.log2_parallel_merge_minus2 as u8;
        pp.curr_pic_order_cnt_val = poc;
        pp.ref_pic_list = [0xFF; 15];
        for (i, d) in self.dpb.iter().take(15).enumerate() {
            pp.ref_pic_list[i] = d.slot as u8 | if d.long_term { 0x80 } else { 0 };
            pp.poc_list[i] = d.poc;
        }
        let idx_of = |dpb_pos: usize| -> u8 { if dpb_pos < 15 { dpb_pos as u8 } else { 0xFF } };
        pp.st_curr_before = [0xFF; 8];
        pp.st_curr_after = [0xFF; 8];
        pp.lt_curr = [0xFF; 8];
        for (i, &p) in refs.0.iter().take(8).enumerate() {
            pp.st_curr_before[i] = idx_of(p);
        }
        for (i, &p) in refs.1.iter().take(8).enumerate() {
            pp.st_curr_after[i] = idx_of(p);
        }
        for (i, &p) in refs.2.iter().take(8).enumerate() {
            pp.lt_curr[i] = idx_of(p);
        }
        self.status_counter = self.status_counter.wrapping_add(1);
        pp.status_report = self.status_counter;

        // Bitstream: je Slice Startcode 00 00 01 + NAL; Ende auf 128 Byte aufgefüllt
        let mut bs: Vec<u8> = vec![];
        let mut sl: Vec<SliceShort> = vec![];
        for nal in slices {
            let pos = bs.len() as u32;
            bs.extend_from_slice(&[0, 0, 1]);
            bs.extend_from_slice(nal);
            sl.push(SliceShort { bs_nal_unit_data_location: pos, slice_bytes_in_buffer: (bs.len() as u32) - pos, bad_slice_chopping: 0 });
        }
        let pad = (128 - (bs.len() & 127)) & 127;
        bs.resize(bs.len() + pad, 0);
        if let Some(last) = sl.last_mut() {
            last.slice_bytes_in_buffer += pad as u32;
        }

        unsafe {
            let mut tries = 0;
            loop {
                match self.vctx.DecoderBeginFrame(&self.decoder, &self.views[slot], 0, None) {
                    Ok(()) => break,
                    Err(e) => {
                        tries += 1;
                        if tries > 100 {
                            return Err(format!("DecoderBeginFrame: {e}"));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }
            let mut descs: Vec<D3D11_VIDEO_DECODER_BUFFER_DESC> = vec![];
            let mut put = |ty: D3D11_VIDEO_DECODER_BUFFER_TYPE, bytes: &[u8]| -> Result<(), String> {
                let mut size = 0u32;
                let mut ptr = std::ptr::null_mut();
                self.vctx.GetDecoderBuffer(&self.decoder, ty, &mut size, &mut ptr).map_err(|e| format!("GetDecoderBuffer: {e}"))?;
                if (size as usize) < bytes.len() {
                    let _ = self.vctx.ReleaseDecoderBuffer(&self.decoder, ty);
                    return Err(format!("Puffer zu klein ({size} < {})", bytes.len()));
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
                self.vctx.ReleaseDecoderBuffer(&self.decoder, ty).map_err(|e| e.to_string())?;
                descs.push(D3D11_VIDEO_DECODER_BUFFER_DESC { BufferType: ty, DataOffset: 0, DataSize: bytes.len() as u32, ..Default::default() });
                Ok(())
            };
            let pp_bytes = std::slice::from_raw_parts(&pp as *const PicParams as *const u8, size_of::<PicParams>());
            let sl_bytes = std::slice::from_raw_parts(sl.as_ptr() as *const u8, sl.len() * size_of::<SliceShort>());
            let qm = default_qmatrix();
            let r = put(D3D11_VIDEO_DECODER_BUFFER_PICTURE_PARAMETERS, pp_bytes)
                .and_then(|_| if sps.scaling_list { put(D3D11_VIDEO_DECODER_BUFFER_INVERSE_QUANTIZATION_MATRIX, &qm) } else { Ok(()) })
                .and_then(|_| put(D3D11_VIDEO_DECODER_BUFFER_SLICE_CONTROL, sl_bytes))
                .and_then(|_| put(D3D11_VIDEO_DECODER_BUFFER_BITSTREAM, &bs));
            if let Err(e) = r {
                let _ = self.vctx.DecoderEndFrame(&self.decoder);
                return Err(e);
            }
            // Größe des Bitstream-Puffers angeben
            if let Some(d) = descs.last_mut() {
                d.DataSize = bs.len() as u32;
            }
            let sub = self.vctx.SubmitDecoderBuffers(&self.decoder, &descs);
            let end = self.vctx.DecoderEndFrame(&self.decoder);
            sub.map_err(|e| format!("SubmitDecoderBuffers: {e}"))?;
            end.map_err(|e| format!("DecoderEndFrame: {e}"))?;
        }
        Ok(())
    }

    /// Liest eine decodierte Oberfläche zurück und wandelt sie in RGBA (Abtastschritt `step`).
    pub fn read_rgba(&self, slot: usize, step: usize) -> Option<(usize, usize, Vec<u8>)> {
        let sps = &self.sps;
        let step = step.max(1);
        unsafe {
            self.ctx.CopySubresourceRegion(&self.staging, 0, 0, 0, 0, &self.texture, slot as u32, None);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut m)).ok()?;
            let pitch = m.RowPitch as usize;
            let base = m.pData as *const u8;
            let (cl, ct) = (sps.crop[0] as usize, sps.crop[2] as usize);
            let w = (sps.width - sps.crop[0] - sps.crop[1]) as usize;
            let h = (sps.height - sps.crop[2] - sps.crop[3]) as usize;
            let (ow, oh) = (w / step, h / step);
            let wide = self.format == DXGI_FORMAT_P010;
            let uv_base = base.add(pitch * self.aligned_h as usize);
            let mut out = vec![255u8; ow * oh * 4];
            let conv = crate::hevc::Conv::new(sps.full_range, sps.matrix, w).with_transfer(sps.transfer);
            for oy in 0..oh {
                let y = ct + oy * step;
                for ox in 0..ow {
                    let x = cl + ox * step;
                    let (yy, u, v) = if wide {
                        let yv = (base.add(y * pitch + x * 2) as *const u16).read_unaligned() >> 8;
                        let uvp = uv_base.add((y / 2) * pitch + (x & !1) * 2) as *const u16;
                        (yv as i32, (uvp.read_unaligned() >> 8) as i32, (uvp.add(1).read_unaligned() >> 8) as i32)
                    } else {
                        let yv = *base.add(y * pitch + x);
                        let uvp = uv_base.add((y / 2) * pitch + (x & !1));
                        (yv as i32, *uvp as i32, *uvp.add(1) as i32)
                    };
                    let d = (oy * ow + ox) * 4;
                    out[d..d + 3].copy_from_slice(&conv.rgb(yy, u, v));
                }
            }
            self.ctx.Unmap(&self.staging, 0);
            Some((ow, oh, out))
        }
    }
}

impl Drop for GpuDecoder {
    fn drop(&mut self) {
        let _ = &self.device;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_reader_exp_golomb() {
        // 1 | 010 | 011 | 00100 → ue: 0, 1, 2, 3
        let d = [0b1010_0110, 0b0100_0000];
        let mut b = Bits::new(&d);
        assert_eq!((b.ue(), b.ue(), b.ue(), b.ue()), (Some(0), Some(1), Some(2), Some(3)));
        // se: 010 → +1, 011 → −1, 00100 → +2
        let d = [0b0100_1100, 0b1000_0000];
        let mut b = Bits::new(&d);
        assert_eq!((b.se(), b.se(), b.se()), (Some(1), Some(-1), Some(2)));
        assert_eq!(Bits::new(&[]).u(1), None);
    }

    #[test]
    fn unescape_removes_emulation_prevention() {
        assert_eq!(unescape(&[1, 0, 0, 3, 1, 0, 0, 3, 0, 5]), vec![1, 0, 0, 1, 0, 0, 0, 5]);
        assert_eq!(unescape(&[0, 0, 4, 3]), vec![0, 0, 4, 3]);
    }

    #[test]
    fn ceil_log2_values() {
        assert_eq!([ceil_log2(1), ceil_log2(2), ceil_log2(3), ceil_log2(4), ceil_log2(5), ceil_log2(1920)], [0, 1, 2, 2, 3, 11]);
    }

    #[test]
    fn default_qmatrix_has_dxva_size() {
        assert_eq!(default_qmatrix().len(), 1000);
    }

    #[test]
    fn dxva_struct_has_spec_size() {
        assert_eq!(size_of::<PicParams>(), 232);
        assert_eq!(size_of::<SliceShort>(), 12);
    }

    #[test]
    fn st_rps_direct_and_inter_prediction() {
        // Direkt: 2 negative (−1 benutzt, −3 nicht benutzt), 1 positive (+2 benutzt)
        // ue(2)=011, ue(1)=010, [ue(0)=1 used=1], [ue(1)=010 used=0], [ue(1)=010 used=1]
        let bits = "011 010 1 1 010 0 010 1";
        let mut bytes = vec![];
        let s: String = bits.chars().filter(|c| !c.is_whitespace()).collect();
        for chunk in s.as_bytes().chunks(8) {
            let mut v = 0u8;
            for (i, c) in chunk.iter().enumerate() {
                if *c == b'1' {
                    v |= 1 << (7 - i);
                }
            }
            bytes.push(v);
        }
        let mut b = Bits::new(&bytes);
        let r = parse_st_rps(&mut b, 0, 1, &[]).unwrap();
        assert_eq!(r.neg, vec![(-1, true), (-3, false)]);
        assert_eq!(r.pos, vec![(2, true)]);
    }

    fn sample_params() -> Option<(Vec<Vec<u8>>, usize)> {
        let dir = crate::app::videos_dir()?;
        let rd = std::fs::read_dir(dir.join("test_vsp")).ok()?;
        for p in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
            if let Ok(t) = crate::hevc::parse_file(&p) {
                return Some((t.params, t.nal_len));
            }
        }
        None
    }

    #[test]
    fn parses_real_parameter_sets() {
        let Some((params, _)) = sample_params() else { return };
        let mut got_sps = false;
        for nal in &params {
            match (nal[0] >> 1) & 0x3F {
                33 => {
                    let s = parse_sps(nal).expect("SPS");
                    assert!(s.width >= 1280 && s.height >= 720 && s.log2_max_poc_lsb >= 4 && s.ctb_log2() >= 4, "{s:?}");
                    assert!(s.pic_size_in_ctbs() > 100);
                    got_sps = true;
                }
                34 => {
                    parse_pps(nal).expect("PPS");
                }
                _ => {}
            }
        }
        assert!(got_sps);
    }

    #[test]
    fn diag_iphone() {
        let Some(dir) = crate::app::videos_dir() else { return };
        let f = dir.join("test_vsp").join("IMG_5844.MOV");
        let Ok(t) = crate::hevc::parse_file(&f) else { return };
        let mut out = String::new();
        for nal in &t.params {
            let ty = (nal[0] >> 1) & 0x3F;
            out += &format!("NAL type {ty} len {}
", nal.len());
            if ty == 33 {
                match parse_sps_raw(nal) {
                    Some(s) => out += &format!("SPS gelesen {s:?}
"),
                    None => out += "SPS nicht lesbar
",
                }
            }
            if ty == 34 {
                out += &format!("PPS {:?}
", parse_pps(nal).is_some());
            }
        }
        out += &format!("GPU new: {}
", GpuDecoder::new(&t.params, t.nal_len).is_some());
        std::fs::write(std::env::temp_dir().join("diag_iphone.txt"), out).unwrap();
    }
}
