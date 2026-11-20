// Velo's Syphon server (share.rs, Share::Syphon): the program's frames, BGRA rows as the share's
// read-back gives them, copied into the server's IOSurface and published. A plain subclass of
// Syphon's SyphonServerBase (third_party/syphon): no Metal or OpenGL, the frame is already on the CPU.

#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#import <stdatomic.h>
#import "SyphonServerBase.h"
#import "SyphonClientBase.h"
#import "SyphonSubclassing.h"

@interface VeloSyphonServer : SyphonServerBase
@end

@implementation VeloSyphonServer
@end

// A server other apps list as `name`; NULL if it couldn't start.
void *velo_syphon_start(const char *name) {
    @autoreleasepool {
        NSString *named = [NSString stringWithUTF8String:name];
        VeloSyphonServer *server = [[VeloSyphonServer alloc] initWithName:named options:nil];
        return server ? (__bridge_retained void *)server : NULL;
    }
}

// One frame: `height` rows of `width` BGRA pixels, `stride` bytes apart. 1 when it went out.
int velo_syphon_publish(void *server, const uint8_t *pixels, size_t width, size_t height, size_t stride) {
    @autoreleasepool {
        VeloSyphonServer *s = (__bridge VeloSyphonServer *)server;
        // the same surface again while the size holds, a new one when it changes
        IOSurfaceRef surface = [s newSurfaceForWidth:width height:height options:nil];
        if (!surface) {
            return 0;
        }
        IOSurfaceLock(surface, 0, NULL);
        uint8_t *base = IOSurfaceGetBaseAddress(surface);
        size_t row = IOSurfaceGetBytesPerRow(surface);
        for (size_t y = 0; y < height; y++) {
            memcpy(base + y * row, pixels + y * stride, width * 4);
        }
        IOSurfaceUnlock(surface, 0, NULL);
        CFRelease(surface);
        [s publish];
        return 1;
    }
}

// The server gone: clients see it leave.
void velo_syphon_stop(void *server) {
    @autoreleasepool {
        VeloSyphonServer *s = (__bridge_transfer VeloSyphonServer *)server;
        [s stop];
    }
}

// For a test: a client of `server`, as another app would be; after `publish` has sent a frame it
// connected for, the first pixel of what it sees (BGRA in a u32, B lowest), or 0 if none came in
// `ms`.
uint32_t velo_syphon_client_sees(void *server, void (*publish)(void *), void *context, int ms) {
    @autoreleasepool {
        VeloSyphonServer *s = (__bridge VeloSyphonServer *)server;
        __block atomic_bool got = false;
        SyphonClientBase *client = [[SyphonClientBase alloc] initWithServerDescription:s.serverDescription options:nil newFrameHandler:^(id c) {
            atomic_store(&got, true);
        }];
        uint32_t seen = 0;
        for (int waited = 0; waited < ms && !atomic_load(&got); waited += 10) {
            publish(context);
            usleep(10000);
        }
        IOSurfaceRef surface = atomic_load(&got) ? [client newSurface] : NULL;
        if (surface) {
            IOSurfaceLock(surface, kIOSurfaceLockReadOnly, NULL);
            seen = *(const uint32_t *)IOSurfaceGetBaseAddress(surface);
            IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, NULL);
            CFRelease(surface);
        }
        [client stop];
        return seen;
    }
}
