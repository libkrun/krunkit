# krunkit Command Line

`krunkit` can launch configurable virtual machines using macOS's hypervisor framework and the `libkrun` virtual
machine monitor library. The `libkrun` virtual machine configuration can be specified from command line arguments.

Specifying a virtual machine's vCPU and RAM allocation is required. Adding devices is optional, yet most workloads
will require a root disk to be useful.

## Generic Options

- `--krun-log-level`

Set the log level for libkrun. Supported values: 0=off, 1=error, 2=warn, 3=info (default), 4=debug, 5 or more=trace.

- `--log-file`

Specify a file path in which libkrun and krunkit will write the logs. The default is stderr. Note, logs will append to an existing file and not truncate it.

- `--restful-uri`

The URI (address) of the RESTful service. If not specified, defaults to `none://`. Valid schemes are
`tcp`, `none`, or `unix`. A scheme of `none` disables the RESTful service.

- `--pidfile`

Specify a path in which krunkit will write the PID to. The option does not provide any form of locking.

- `--timesync`

Specify a Vsock port on which the host will send the current time to
the `qemu-guest-agent` process running inside the guest.

#### Example

```bash
--timesync vsockPort=1234
```
On guest:
```bash
 /usr/bin/qemu-ga --method=vsock-listen --path=3:1234 --logfile=/var/log/qemu-ga.log -v
```

### Virtual Machine Resources

- `--cpus`

Number of vCPUs available to a virtual machine.

- `--memory`

Amount of RAM available to a virtual machine. Value is in MiB (mebibytes, 1024^2 bytes).

- `--nested`

Enable Nested Virtualization.

Note: this requires an M3 or newer CPU and macOS 15+.

#### Example

This configures a virtual machine to use two vCPUs and 2048 MiB of RAM:

```
--cpus 2 --memory 2048
```

## Bootloader Configuration

### EFI bootloader

`--bootloader efi` allows booting a disk image using EFI, which removes the need for providing external kernel/initrd/... jthe disk image bootloader will be started by the EFI firmware, which will in turn know which kernel it should be booting.

#### Arguments
- `variable-store`: path to a file which EFI can use to store its variables
- `create`: indicate whether the `variable-store` file hould be created or not if missing.

> [!NOTE]
> This option is ignored by the commandline. It is added purely for vfkit parity. `krunkit` only supports the EFI bootloader configuration and handles all associated actions without further user configuration.

## Device Configuration

Various virtio devices can be added to a virtual machine. They are all paravirtualized devices that can be
specified using the `--device` flag.

### Disk

The `virtio-blk` option adds a disk to a virtual machine. This disk is backed by an image file on the host
machine. At least one virtio-blk device must be specified on the commandline. The first virtio-blk argument
will be used as a virtual machine's root disk (`/dev/vda`). The subsequent virtio-blk arguments will be used
as a virtual machine's data disk(s) (`/dev/vd[b-z]`).

#### Arguments

- `path`: Path to the disk image file.
- `format`: Format of the disk image. Supported formats: raw, qcow2.

#### Example

This adds a virtio-blk device to a virtual machine which will be backed by an image at
`/Users/user/disk-image.raw`:

```
--device virtio-blk,path=/Users/user/disk-image.raw,format=raw
```

### Networking

The `virtio-net` option adds a network interface to a virtual machine.

#### Arguments

Arguments to create a virtio-net device that has offloading enabled by default and will send a VFKIT magic
value after establishing the connection:
- `unixSocketPath`: Path to a UNIX socket to attach to the guest network interface.
- `mac`: MAC address of a virtual machine.

Arguments to create a virtio-net device with a unix datagram or unix stream socket backend:
- `type`: Unix socket type:
    - `unixgram`: unix datagram socket-based backend such as gvproxy or vmnet-helper.
    - `unixstream`: unix stream socket-based userspace network proxy such as passt or socket_vmnet.
- `path`: Unix socket path. Mutually exclusive with the `fd=` option.
- `fd`: Unix socket file descriptor. Mutually exclusive with the `path=` option.
- `mac`: MAC address of a virtual machine.
- `offloading`: (Optional) Whether or not to enable network offloading between the guest and host.
Default value is `off`.
- `vfkitMagic`: (Optional) Whether to send the vfkit magic value after establishing a network connection.
Default value is `off`. Only supported with the `unixgram` type.

> [!NOTE]
> The `unixSocketPath`, `type={unixgram, unixstream}` arguments are mutually exclusive and cannot be used together.

#### Example

If you want to use a tool like vmnet-helper as your backend, you'll be interested in using the `type=unixgram`
argument. This will create a virtio-net device and redirect all guest network traffic to the corresponding socket.
Here, we're not going to use any of the optional arguments. Therefore, offloading will be disabled by default
and the vfkit magic value won't be sent when the connection is established:

```
--device virtio-net,type=unixgram,path=/Users/user/vm-network.sock,mac=ff:ff:ff:ff:ff:ff
```

If you want to use gvproxy instead, you're going to want to use some of the optional arguments krunkit provides.
We're going to want to enable offloading, and if gvproxy is running in vfkit mode, we'll also want to send the
vfkit magic value when the connection establishes:

```
--device virtio-net,type=unixgram,path=/Users/user/vm-network.sock,mac=ff:ff:ff:ff:ff:ff,offloading=on,vfkitMagic=on
```

You can also use a network proxy such as passt by using the `type=unixstream` argument:
```
--device virtio-net,type=unixstream,fd=<passt_fd>,mac=ff:ff:ff:ff:ff:ff,offloading=true
```

To see performance implications of choosing offloading vs. not offloading, see [this table](#offloading-performance-implications)

### Serial Port

The `virtio-serial` option adds a serial device to a virtual machine. This allows for redirection of virtual
machine text output.

#### Arguments

- `logFilePath`: Path to a file in which a virtual machine's serial port output should be written.

#### Example

This adds a virtio-serial device to a virtual machine, and will redirect the virtual machine's text output to
`/Users/user/vm-output.log`:

```
--device virtio-serial,logFilePath=/Users/user/vm-output.log
```

### vsock

The `virtio-vsock` option adds a vsock communication channel between the host and guest. macOS does not have host
support for `AF_VSOCK` sockets, so the virtual machine monitor will maintain a vsock-UNIX socket proxy to
facilitate communication between the two.

Multiple instances of a `virtio-vsock` device can be specified, yet port numbers for these sockets must be unique.

There are two modes - `listen` and `connect`.

#### Arguments

- `port`: `AF_VSOCK` port to connect to on the guest.
- `socketURL`: Path to the UNIX socket on the host.
- `listen`: Enable guest to host communication. (Default mode)
- `connect`: Enable host to guest communication.

#### Example

This adds a virtio-vsock device to a virtual machine, and will forward all guest socket communication to
`/Users/user/vm-socket.sock` (a virtual machine can connect to the vsock on port `1024`):

```
--device virtio-vsock,port=1024,socketURL=/Users/user/vm-socket.sock
```

To enable host-initiated connections to the guest:

```
--device virtio-vsock,port=1024,socketURL=/Users/user/vm-socket.sock,connect
```

### File Sharing

The `virtio-fs` option allows a guest to share a file system directory with a host. The directory can be mounted
in the guest with `mount -t virtiofs MOUNT_TAG /mnt`, with `MOUNT_TAG` corresponding to the mount tag specified in
the arguments.

#### Arguments

- `sharedDir`: Path to the host directory that will be shared with the guest.
- `mountTag`: Tag to be used to mount the shared directory in the guest.
- `permissionSemantics`: (Optional) Permission semantics to be used in the virtio-fs device. Supported values: `complete`, `simplified`. Default value is `simplified`.

#### Example

This will share `/Users/user/shared-dir` with the guest:

```
--device virtio-fs,sharedDir=/Users/user/shared-dir,mountTag=MOUNT_TAG
```

To use complete permission semantics:

```
--device virtio-fs,sharedDir=/Users/user/shared-dir,mountTag=MOUNT_TAG,permissionSemantics=complete
```

## Restful Service

Recall that the RESTful service is started at the address specified in the `--restful-uri` argument (or
`tcp://localhost:8081` if not specified).

### Getting a virtual machine's state

Used to obtain the state of a running virtual machine.

`GET /vm/state`

Response: `VirtualMachineState{Running, Stopped}`

### Stopping a virtual machine

`POST /vm/state` `{ "state": "Stop" }`

Response: `VirtualMachineStateStopped`

## Offloading Performance Implications

The table below provides some data on how offloading effects the gvproxy and vmnet-helper backends:

#### vment-helper offloading

| network       | vm       | offloading | iper3         | iperf3 -R     |
|---------------|--------- |------------|---------------|---------------|
| vmnet-helper  | krunkit  | true       |  1.38 Gbits/s | 46.20 Gbits/s |
| vmnet-helper  | krunkit  | false      | 10.10 Gbits/s |  8.38 Gbits/s |
| vmnet-helper  | vfkit    | true       |  4.27 Gbits/s |  8.09 Gbits/s |
| vmnet-helper  | vfkit    | false      | 10.70 Gbits/s |  8.41 Gbits/s |

#### gvproxy offloading

| network       | vm       | offloading | iper3         | iperf3 -R     |
|---------------|--------- |------------|---------------|---------------|
| gvproxy       | krunkit  | true       |  1.40 Gbits/s | 20.00 Gbits/s |
| gvproxy       | krunkit  | false      |  1.47 Gbits/s |  2.58 Gbits/s |
| gvproxy       | vfkit    | false      |  1.43 Gbits/s |  2.84 Gbits/s |

