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
