# E8: what the accelerated display device must do

Date: 2026-11-11. A working specification for bvmm's virtio-gpu device with
gfxstream behind it, in "guest ANGLE" mode (Vulkan on the host, OpenGL ES
inside Android).

Confidence: read from source by a research pass on 2026-11-11 unless marked
(inferred). Sources: Linux v6.12 `include/uapi/linux/virtio_gpu.h` and
`drivers/gpu/drm/virtio`; crosvm f3bca5f `devices/src/virtio/gpu`;
rutabaga_gfx d953019; gfxstream b8537e6 `host/`; android-cuttlefish 0a2e657;
gfxstream's guest code as mirrored in Mesa. minigbm could not be read, so
statements about it are inferred. Nothing here has been run yet.

## Device features and configuration

Feature bits: VIRGL 0, EDID 1, RESOURCE_UUID 2, RESOURCE_BLOB 3,
CONTEXT_INIT 4. Offer VIRGL, RESOURCE_BLOB and CONTEXT_INIT (the kernel
needs both VIRGL and CONTEXT_INIT for context creation). Do not offer bit 16
(crosvm's private "create guest handle").

Configuration (16 bytes): events_read@0, events_clear@4, num_scanouts@8,
num_capsets@12. `num_capsets` = 2.

Capability sets, by index: 0 is id 3 (GFXSTREAM_VULKAN, 116 bytes), 1 is
id 9 (GFXSTREAM_COMPOSER, 16 bytes). Version 0 for both. An index out of
range answers with id `u32::MAX`.

## Control header (24 bytes)

type u32@0, flags u32@4, fence_id u64@8, ctx_id u32@16, ring_idx u8@20.
Flags: FENCE = 1, INFO_RING_IDX = 2.

## Commands (offsets include the header)

| Command | Type | Body | Size |
|---|---|---|---|
| GET_CAPSET_INFO | 0x0108 | capset_index@24 | 32 |
| GET_CAPSET | 0x0109 | capset_id@24, capset_version@28 | 32 |
| RESOURCE_ASSIGN_UUID | 0x010b | resource_id@24 | 32 |
| RESOURCE_CREATE_BLOB | 0x010c | resource_id@24, blob_mem@28, blob_flags@32, nr_entries@36, blob_id u64@40, size u64@48, then entries {addr u64, length u32, pad u32} | 56+16n |
| SET_SCANOUT_BLOB | 0x010d | rect x@24 y@28 w@32 h@36, scanout_id@40, resource_id@44, width@48, height@52, format@56, strides[4]@64, offsets[4]@80 | 96 |
| CTX_CREATE | 0x0200 | nlen@24, context_init@28 (capset id in the low byte), debug_name[64]@32 | 96 |
| CTX_DESTROY | 0x0201 | (ctx_id in the header) | 24 |
| CTX_ATTACH_RESOURCE / DETACH | 0x0202 / 0x0203 | resource_id@24 | 32 |
| RESOURCE_CREATE_3D | 0x0204 | resource_id@24, target@28, format@32, bind@36, width@40, height@44, depth@48, array_size@52, last_level@56, nr_samples@60, flags@64 | 72 |
| TRANSFER_TO_HOST_3D / FROM | 0x0205 / 0x0206 | box x@24 y@28 z@32 w@36 h@40 d@44, offset u64@48, resource_id@56, level@60, stride@64, layer_stride@68 | 72 |
| SUBMIT_3D | 0x0207 | size@24, then `size` bytes | 32+size |
| RESOURCE_MAP_BLOB | 0x0208 | resource_id@24, offset u64@32 | 40 |
| RESOURCE_UNMAP_BLOB | 0x0209 | resource_id@24 | 32 |

Responses: OK_NODATA 0x1100; CAPSET_INFO 0x1102 (capset_id@24,
max_version@28, max_size@32; 40 bytes); CAPSET 0x1103 (header then data);
RESOURCE_UUID 0x1105 (uuid[16]@24); MAP_INFO 0x1106 (map_info u32@24; 32
bytes). Errors: UNSPEC 0x1200, OUT_OF_MEMORY 0x1201, INVALID_SCANOUT 0x1202,
INVALID_RESOURCE 0x1203, INVALID_CONTEXT 0x1204, INVALID_PARAMETER 0x1205.

blob_mem: GUEST 1, HOST3D 2, HOST3D_GUEST 3. blob_flags: MAPPABLE 1,
SHAREABLE 2, CROSS_DEVICE 4. Map cache: NONE 0, CACHED 1, UNCACHED 2, WC 3.

## What each command does to the renderer

| Command | Renderer call |
|---|---|
| GET_CAPSET_INFO | `get_cap_set(id)`; gfxstream leaves the version at 0 |
| GET_CAPSET | `fill_caps(id, version, buffer of max_size)` |
| CTX_CREATE | `context_create(ctx, nlen, name, context_init)`; name "gpu_renderer" if empty |
| CTX_DESTROY | `context_destroy(ctx)` |
| CTX_ATTACH / DETACH_RESOURCE | `ctx_attach_resource` / `ctx_detach_resource` |
| RESOURCE_CREATE_3D | `resource_create(args, no iovecs)` |
| RESOURCE_CREATE_2D | the same with target 2, bind 2, depth 1, array_size 1 |
| ATTACH_BACKING | `resource_attach_iov` with the host addresses of the guest's pages |
| DETACH_BACKING | `resource_detach_iov(resource, null, null)` |
| TRANSFER_TO_HOST_3D | `transfer_write_iov(resource, ctx, level, stride, layer_stride, box, offset, no iovecs)` |
| TRANSFER_FROM_HOST_3D | `transfer_read_iov(..., no iovecs)` |
| TRANSFER_TO_HOST_2D | the same write with ctx 0 and box {x, y, 0, w, h, 1} |
| SUBMIT_3D | `submit_cmd`; size must be a multiple of 4; size 0 answers OK with no call |
| RESOURCE_CREATE_BLOB | `create_blob(ctx, resource, {blob_mem, blob_flags, blob_id, size}, iovecs unless HOST3D)`; ctx 0 with GUEST memory is only recorded, never sent |
| RESOURCE_MAP_BLOB | `resource_map` gives a host address and size; map it at window base + offset; answer MAP_INFO with `resource_map_info & 0x0f` |
| RESOURCE_UNMAP_BLOB | unmap from the guest, then `resource_unmap` |
| RESOURCE_ASSIGN_UUID | none; uuid is 12 zero bytes then the resource id, big-endian |
| SET_SCANOUT, SET_SCANOUT_BLOB | none; the device remembers which resource is on screen |
| RESOURCE_FLUSH | none on Linux; read the picture back (below) |
| RESOURCE_UNREF | `resource_unref` |

Any non-zero return is a failure; some paths return positive error numbers.

## Fences

For a command with FLAG_FENCE: run the command, then

- without INFO_RING_IDX: `create_fence` on the global ring;
- with INFO_RING_IDX and ring_idx 1: complete at once, no renderer call;
- with INFO_RING_IDX otherwise: `create_fence` with the ring flag.

The response to a fenced command (its header echoing flags, fence_id, ctx_id
and ring_idx) is **held until the renderer's fence callback** for that ring
reports a fence_id at or beyond it. The device keeps the last completed
fence per ring and answers at once if the fence is already behind it. The
callback may run inside `create_fence` or later on a renderer thread, so
the device's lock must not be held across `create_fence`.

## Blobs and the shared-memory window

The guest creates two kinds, both HOST3D and MAPPABLE, both of which must
be mapped into the guest:

- the command ring (blob_id 0, about 1 MiB), followed by a fenced SUBMIT_3D
  and a MAP_BLOB;
- Vulkan host-visible memory (blob_id from the host).

Renderer flags: neither "external blob" nor "system blob". `resource_map`
then returns a plain host address, which is all a memory slot or
`WHvMapGpaRange` can use. gfxstream requires that address to be 4 KiB
aligned; on Windows it arranges that itself, on Linux the renderer feature
`VulkanAllocateHostMemory:enabled` may be needed (inferred).

The window: PCI capability type 8, 24 bytes long, BAR 2 (64-bit,
prefetchable), id 1, offset 0, 8 GiB. MAP_BLOB's offset is from the start of
the window.

Graphics buffers from Android's allocator are ordinary RESOURCE_CREATE_3D
resources with attached backing and 3D transfers (inferred).

## Getting the picture

Android's composer draws on the host and then commits a buffer: the kernel
sends SET_SCANOUT (or SET_SCANOUT_BLOB) and RESOURCE_FLUSH. The buffer's
pixels are on the host; the guest's copy of them is stale.

On RESOURCE_FLUSH of the scanout resource: `transfer_read_iov(resource,
ctx 0, level 0, stride, 0, box {0,0,0,w,h,1}, offset 0, one iovec of w*h*4
bytes)`. Rows come out tightly packed. This works only for resources that
have had backing attached, which the allocator's buffers have.

## Starting gfxstream

Flags: surfaceless (1<<3) and Vulkan (1<<5). Leaving out the GLES flag makes
it Vulkan-only. Optional renderer features are a string parameter (key 11),
`"Name:enabled,Name:disabled"`; an unknown name fails the start.

**Known trap:** if Vulkan cannot start, `stream_renderer_init` waits for
ever instead of returning. Some internal inconsistencies abort the process.
gameDroid checks in a separate process first (`gamedroid-gpu-check`).

## Android's boot settings for this mode

    androidboot.cpuvulkan.version = 0
    androidboot.hardware.gralloc = minigbm
    androidboot.hardware.hwcomposer = ranchu
    androidboot.hardware.hwcomposer.display_finder_mode = drm
    androidboot.hardware.hwcomposer.display_framebuffer_format = rgba
    androidboot.hardware.egl = angle
    androidboot.hardware.vulkan = ranchu
    androidboot.hardware.gltransport = virtio-gpu-asg
    androidboot.opengles.version = 196609
    androidboot.hardware.angle_feature_overrides_enabled = mapUnspecifiedColorSpaceToPassThrough

Cuttlefish adds the renderer feature `VulkanUseDedicatedAhbMemoryType:enabled`
when the host's Vulkan is SwiftShader, and
`angle_feature_overrides_disabled = enablePrecisionQualifiers` for an NVIDIA
quirk.
