#import <AppKit/AppKit.h>
#import <objc/runtime.h>

// CEF requires these protocols on the existing NSApplication. The category
// preserves Tao's application class, delegate, menu and event-loop ownership.
@protocol CrAppProtocol
- (BOOL)isHandlingSendEvent;
@end
@protocol CrAppControlProtocol <CrAppProtocol>
- (void)setHandlingSendEvent:(BOOL)value;
@end
@protocol CefAppProtocol <CrAppControlProtocol>
@end
static _Thread_local BOOL handlingEvent;
static BOOL pumping;
static NSTimer *pumpTimer;
@interface NSApplication (MunimentCEF) <CefAppProtocol>
- (void)munimentCEFSendEvent:(NSEvent *)event;
- (void)munimentCEFTerminate:(id)sender;
@end
@implementation NSApplication (MunimentCEF)
- (BOOL)isHandlingSendEvent { return handlingEvent; }
- (void)setHandlingSendEvent:(BOOL)value { handlingEvent = value; }
- (void)munimentCEFSendEvent:(NSEvent *)event {
    BOOL previous = handlingEvent;
    handlingEvent = YES;
    @try { [self munimentCEFSendEvent:event]; }
    @finally { handlingEvent = previous; }
}
- (void)munimentCEFTerminate:(id)sender {
    if (pumping) {
        dispatch_async(dispatch_get_main_queue(), ^{ [self munimentCEFTerminate:sender]; });
    } else {
        [self munimentCEFTerminate:sender];
    }
}
@end
void muniment_cef_install_app_protocol(void) {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        method_exchangeImplementations(class_getInstanceMethod(NSApplication.class, @selector(terminate:)), class_getInstanceMethod(NSApplication.class, @selector(munimentCEFTerminate:)));
        method_exchangeImplementations(class_getInstanceMethod(NSApplication.class, @selector(sendEvent:)), class_getInstanceMethod(NSApplication.class, @selector(munimentCEFSendEvent:)));
    });
}
void *muniment_cef_parent(void *window) {
    return (__bridge void *)[(__bridge NSWindow *)window contentView];
}
void muniment_cef_bounds(void *handle, double x, double y, double width, double height, BOOL hidden) {
    NSView *view = (__bridge NSView *)handle;
    NSView *parent = view.superview;
    if (!parent.isFlipped) y = parent.bounds.size.height - y - height;
    view.frame = NSMakeRect(x, y, width, height);
    view.hidden = hidden;
}

// A native timer runs outside Tao's callback lock. CEF can then dispatch menu
// actions without reentering the callback that queued its work.
void muniment_cef_start_pump(void (*callback)(void)) {
    pumpTimer = [NSTimer timerWithTimeInterval:0.01 repeats:YES block:^(NSTimer *timer) {
        if (pumping) return;
        pumping = YES;
        @try { callback(); } @finally { pumping = NO; }
    }];
    [[NSRunLoop mainRunLoop] addTimer:pumpTimer forMode:NSRunLoopCommonModes];
    [[NSRunLoop mainRunLoop] addTimer:pumpTimer forMode:NSEventTrackingRunLoopMode];
}
void muniment_cef_stop_pump(void) {
    [pumpTimer invalidate];
    pumpTimer = nil;
}
