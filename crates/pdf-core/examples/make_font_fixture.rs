//! Build the Type0/CIDFontType2 fixture PDF for manual subsetting checks.
//! 生成 Type0/CIDFontType2 夹具 PDF，供手动验证字体子集化。
//!
//! Usage: make_font_fixture <output.pdf>

use std::io::Write as _;

fn main() {
    let output = std::env::args()
        .nth(1)
        .expect("usage: make_font_fixture <output.pdf>");
    let bytes = pdf_core::testutil::build_type0_pdf_bytes();
    let mut file = std::fs::File::create(&output).expect("create output");
    file.write_all(&bytes).expect("write output");
    println!("wrote {output}: {} bytes", bytes.len());
}
