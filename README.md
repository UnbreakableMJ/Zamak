# Zamak Bootloader

![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)

**Zamak** is a modern, high-performance bootloader written in Rust for **x86_64** systems. Designed for reliability and simplicity, Zamak provides a Limine-compatible interface while supporting both **UEFI** and **Legacy BIOS** boot flows.

## 🚀 Features

- **Dual-Mode Booting**: Seamless support for modern UEFI firmware and Legacy BIOS (MBR).
- **Limine Protocol**: Implements the Limine boot protocol, providing kernels with memory maps, resolution-aware framebuffers, and Higher Half Direct Mapping (HHDM).
- **Rust Powered**: Leverage Rust's safety and modern tooling for low-level systems code.
- **FAT32 Support**: Built-in read-only FAT32 driver for locating kernels and configuration files across directory hierarchies.
- **ELF64 Loader**: Robust parsing and loading of 64-bit ELF kernel segments.
- **Long Mode Handover**: Automated transition from 32-bit (BIOS) or UEFI environments to 64-bit Long Mode with paging enabled.

## 📂 Project Structure

- `zamak-loader/`: The UEFI entry point (`zamak.efi`).
- `zamak-bios/`: The BIOS-specific Stage 1 (MBR) and Stage 2 (Rust) loaders.
- `libzamak/`: Shared `no_std` library for protocol handling, ELF parsing, and configuration logic.

## 🛠 Building

### Prerequisites

- [Rust Nightly](https://rust-lang.github.io/rustup/concepts/channels.html)
- `nasm` (for BIOS Stage 1)
- `objcopy` (binary generation)

### Build UEFI Loader
```bash
cargo build -p zamak-loader --release
```

### Build BIOS Image
```bash
make -f Makefile.bios clean all
```
This generates `zamak_bios.img`, a 1MB bootable disk image.

## 📝 Configuration

Zamak looks for a `zamak.con` file on the boot volume. Example configuration:

```ini
TIMEOUT=5

:Zamak OS
    PROTOCOL=limine
    KERNEL_PATH=/boot/kernel
    CMDLINE=some_kernel_parameter=1
```

## ⚖️ License

Zamak is licensed under the **GPL-3.0-or-later** license. See [LICENSE](LICENSE) for details.

---

*Zamak is a work in progress. Contributions and feedback are welcome!*
