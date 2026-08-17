// O coreaudio-sys já linka CoreAudio e AudioToolbox, mas não o CoreFoundation, usado para
// converter caminhos em CFURL e nomes de device em String.
fn main() {
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
}
