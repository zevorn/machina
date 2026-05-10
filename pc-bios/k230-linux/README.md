# K230 Linux Boot Artifacts

These direct Linux boot files are copied from:

```text
~/k230_sdk/output/k230_canmv_defconfig/images/little-core/
```

They are used by the K230 mtest regression to cover direct Linux boot with
the built-in SBI path:

- `Image`
- `k230.dtb`
- `rootfs.cpio.gz`

The SDK U-Boot and SD image artifacts are copied from:

```text
~/k230_sdk/output/k230_canmv_defconfig/little/uboot/u-boot
~/k230_sdk/output/k230_canmv_defconfig/images/sysimage-sdcard.img.gz
```

The raw SD image is stored compressed to keep the repository copy smaller.
The mtest regression decompresses it into a temporary file before attaching it
as the K230 SD1 drive.

To run the SDK U-Boot flow by hand:

```sh
mkdir -p /tmp/machina-k230
gzip -cd pc-bios/k230-linux/sysimage-sdcard.img.gz \
  > /tmp/machina-k230/sysimage-sdcard.img

./target/release/machina -M k230 -m 2048 \
  -bios pc-bios/k230-linux/u-boot \
  -drive file=/tmp/machina-k230/sysimage-sdcard.img \
  -nographic
```

Do not run bare `bootm` at the `K230#` prompt. The SDK loader must first read
the payloads from the SD image, decompress them, and then call `bootm`
internally. If autoboot is interrupted and the prompt appears, run:

```text
run bootcmd
```

The SD image environment sets `bootcmd=k230_boot auto auto_boot;`.

In `-nographic` mode, the host escape keys are prefix sequences. Press and
release `Ctrl+A`, then press `X` to exit Machina. Pressing all keys at the
same time sends a different terminal control character to the guest.
