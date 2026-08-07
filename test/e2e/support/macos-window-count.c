// Counts the on-screen windows one process owns, for the installed-app probe in
// test/e2e/runner/macos.sh.
//
// CGWindowListCopyWindowInfo reports window metadata — owning pid, layer,
// bounds — and macOS asks for no privacy consent to read it. The AppleScript
// probe this replaced drove System Events, which needs both Apple Events and
// Accessibility grants. An ephemeral CI VM cannot answer the consent dialog, so
// every AppleScript call timed out and the smoke gate never read a count.
//
// Two rules keep this probe permission free. It never reads a window name,
// because macOS withholds names without the Screen Recording grant. It never
// captures window contents, which is the one CGWindowList operation that grant
// covers.
//
// Usage: macos-window-count <pid>. The probe prints the count on stdout and
// exits 0. Every failure prints a message on stderr and exits nonzero.

#include <CoreFoundation/CoreFoundation.h>
#include <CoreGraphics/CoreGraphics.h>
#include <errno.h>
#include <limits.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// The caller polls once a second, so one read must never block the loop. This
// bound replaces the `with timeout of 2 seconds` the AppleScript probe used.
#define WINDOW_LIST_TIMEOUT_SECONDS 2

static void report_timeout(int signal_number) {
  (void)signal_number;
  static const char message[] = "window probe timed out reading the window list\n";
  ssize_t written = write(STDERR_FILENO, message, sizeof(message) - 1);
  (void)written;
  _exit(1);
}

static bool read_number(CFDictionaryRef window, CFStringRef key, long *out) {
  CFTypeRef value = CFDictionaryGetValue(window, key);
  if (value == NULL || CFGetTypeID(value) != CFNumberGetTypeID()) return false;
  return CFNumberGetValue((CFNumberRef)value, kCFNumberLongType, out);
}

// The on-screen-only list already excludes hidden windows, so a window that
// omits the key, or carries an unexpected type for it, still counts.
static bool on_screen(CFDictionaryRef window) {
  CFTypeRef value = CFDictionaryGetValue(window, kCGWindowIsOnscreen);
  if (value == NULL || CFGetTypeID(value) != CFBooleanGetTypeID()) return true;
  return CFBooleanGetValue((CFBooleanRef)value);
}

// A window the user can see covers at least one point. This rejects the
// zero-sized windows a framework opens while it starts up.
static bool has_area(CFDictionaryRef window) {
  CFTypeRef bounds = CFDictionaryGetValue(window, kCGWindowBounds);
  if (bounds == NULL || CFGetTypeID(bounds) != CFDictionaryGetTypeID()) return false;
  CGRect rect;
  if (!CGRectMakeWithDictionaryRepresentation((CFDictionaryRef)bounds, &rect)) return false;
  return rect.size.width >= 1.0 && rect.size.height >= 1.0;
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: macos-window-count <pid>\n");
    return 2;
  }
  errno = 0;
  char *end = NULL;
  long pid = strtol(argv[1], &end, 10);
  if (errno != 0 || end == argv[1] || *end != '\0' || pid <= 0 || pid > INT_MAX) {
    fprintf(stderr, "window probe needs one positive process identifier\n");
    return 2;
  }

  struct sigaction action;
  memset(&action, 0, sizeof(action));
  action.sa_handler = report_timeout;
  sigemptyset(&action.sa_mask);
  if (sigaction(SIGALRM, &action, NULL) != 0) {
    fprintf(stderr, "window probe could not arm its timeout\n");
    return 1;
  }
  alarm(WINDOW_LIST_TIMEOUT_SECONDS);

  CFArrayRef windows = CGWindowListCopyWindowInfo(
      kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
  alarm(0);
  if (windows == NULL) {
    fprintf(stderr, "window probe could not read the on-screen window list\n");
    return 1;
  }

  long count = 0;
  CFIndex total = CFArrayGetCount(windows);
  for (CFIndex index = 0; index < total; index++) {
    CFTypeRef entry = CFArrayGetValueAtIndex(windows, index);
    if (entry == NULL || CFGetTypeID(entry) != CFDictionaryGetTypeID()) continue;
    CFDictionaryRef window = (CFDictionaryRef)entry;
    long owner = 0;
    // Match the owning pid, never the process name. A second process with the
    // same name must not satisfy the gate.
    if (!read_number(window, kCGWindowOwnerPID, &owner) || owner != pid) continue;
    long layer = 0;
    // Layer 0 is the normal window level. Higher layers carry menus, tooltips
    // and status items, which are not the first window the smoke waits for.
    if (!read_number(window, kCGWindowLayer, &layer) || layer != 0) continue;
    if (!on_screen(window) || !has_area(window)) continue;
    count++;
  }
  CFRelease(windows);
  printf("%ld\n", count);
  return 0;
}
