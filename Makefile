ISO_X86=boruix-x86.iso

.PHONY: all iso-x86 run-x86 limine

all: iso-x86

limine:
	git clone https://codeberg.org/Limine/Limine.git --branch=v10.x-binary --depth=1 limine || true
	make -C limine

iso-x86: limine
	cd user && cargo build --release --target x86_64-unknown-none
	RUSTFLAGS="-Clink-arg=-Tkernel/linker.ld -Crelocation-model=static" cargo build --release --target x86_64-unknown-none
	rm -rf iso_root
	mkdir -p iso_root
	cp target/x86_64-unknown-none/release/kernel iso_root/kernel.elf
	cp user/target/x86_64-unknown-none/release/initproc iso_root/
	cp user/target/x86_64-unknown-none/release/loop_a iso_root/
	cp user/target/x86_64-unknown-none/release/loop_b iso_root/
	cp user/target/x86_64-unknown-none/release/rogue iso_root/
	cp user/target/x86_64-unknown-none/release/testsmp1 iso_root/
	cp user/target/x86_64-unknown-none/release/race iso_root/
	cp user/target/x86_64-unknown-none/release/ipi iso_root/
	cp user/target/x86_64-unknown-none/release/memstress iso_root/
	cp limine.conf iso_root/
	cp limine/limine-bios.sys iso_root/
	cp limine/limine-bios-cd.bin iso_root/
	cp limine/limine-uefi-cd.bin iso_root/
	mkdir -p iso_root/EFI/BOOT
	cp limine/BOOTX64.EFI iso_root/EFI/BOOT/BOOTX64.EFI
	cp limine/BOOTIA32.EFI iso_root/EFI/BOOT/BOOTIA32.EFI
	xorriso -as mkisofs -b limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table \
        -eltorito-alt-boot -e limine-uefi-cd.bin -no-emul-boot -isohybrid-gpt-basdat \
        -o $(ISO_X86) iso_root
	limine/limine bios-install $(ISO_X86)

run-x86: iso-x86
	qemu-system-x86_64 -m 2G -cdrom $(ISO_X86) -serial stdio -display none
	
run-x86-display: iso-x86
	qemu-system-x86_64 -m 2G -cdrom $(ISO_X86) -serial stdio

run-x86-smp-display: iso-x86
	qemu-system-x86_64 -m 2G -smp 4 -cdrom $(ISO_X86) -serial stdio

run-x86-smp-ide-display: iso-x86
	qemu-system-x86_64 -m 2G -smp 4 -cdrom $(ISO_X86) -serial stdio -drive file=disk.img,format=raw,if=ide -boot order=dc

run-x86-smp-net-display: iso-x86
	qemu-system-x86_64 -m 2G -smp 4 -cdrom $(ISO_X86) -serial stdio -netdev user,id=net0 -device e1000,netdev=net0

run-x86-smp-net-tap-display: iso-x86
	qemu-system-x86_64 -m 2G -smp 4 -cdrom $(ISO_X86) -serial stdio -netdev tap,id=net0,ifname=tap0,script=no,downscript=no -device e1000,netdev=net0

run-x86-smp-minimm-display: iso-x86
	qemu-system-x86_64 -m 512M -smp 4 -cdrom $(ISO_X86) -serial stdio
