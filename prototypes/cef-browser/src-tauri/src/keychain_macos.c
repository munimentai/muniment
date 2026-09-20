// Prototype compatibility bridge until CEF exposes its app-specific OSCrypt names.
// Only the exact Chromium generic-password pair is rewritten. No shared item is
// read, changed, imported, or granted a broader ACL. Remove this bridge when the
// upstream settings are available in the pinned CEF SDK and Rust bindings.
#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <dlfcn.h>
#include <pthread.h>

static CFStringRef service(void) { return CFSTR("com.muniment.cef-prototype.browser-storage"); }
static CFStringRef account(void) { return CFSTR("primary-v2"); }
static FILE *audit_log;
static bool ready;

static bool equals(CFDictionaryRef q, CFStringRef key, CFTypeRef value) {
    CFTypeRef found = CFDictionaryGetValue(q, key);
    return found && CFEqual(found, value);
}
static bool chromium_item(CFDictionaryRef q) {
    return q && equals(q, kSecClass, kSecClassGenericPassword)
        && equals(q, kSecAttrService, CFSTR("Chromium Safe Storage"))
        && equals(q, kSecAttrAccount, CFSTR("Chromium"));
}
static CFMutableDictionaryRef own_query(CFDictionaryRef q) {
    CFMutableDictionaryRef copy = CFDictionaryCreateMutableCopy(NULL, 0, q);
    CFDictionarySetValue(copy, kSecAttrService, service());
    CFDictionarySetValue(copy, kSecAttrAccount, account());
    return copy;
}
static void record(const char *operation, OSStatus status) {
    if (audit_log) {
        fprintf(audit_log, "%s status=%d\n", operation, (int)status);
        fflush(audit_log);
    }
}
static void require_access(OSStatus status) {
    // Do not let Chromium treat an inaccessible key as a missing cookie, create
    // a replacement key, or continue writing an unreadable persistent profile.
    if (status != errSecSuccess) {
        record("storage-unavailable", status);
        _exit(78);
    }
}
// Explicit link dependency, scoped to CEF. Resolve the original APIs directly
// from Security.framework so forwarding never returns to this library.
static pthread_once_t originals_once = PTHREAD_ONCE_INIT;
static OSStatus (*original_copy)(CFDictionaryRef, CFTypeRef *);
static OSStatus (*original_add)(CFDictionaryRef, CFTypeRef *);
static OSStatus (*original_update)(CFDictionaryRef, CFDictionaryRef);
static OSStatus (*original_delete)(CFDictionaryRef);
static void load_originals(void) {
    void *framework = dlopen("/System/Library/Frameworks/Security.framework/Versions/A/Security",
        RTLD_LAZY | RTLD_LOCAL | RTLD_FIRST);
    if (!framework) _exit(78);
    original_copy = dlsym(framework, "SecItemCopyMatching");
    original_add = dlsym(framework, "SecItemAdd");
    original_update = dlsym(framework, "SecItemUpdate");
    original_delete = dlsym(framework, "SecItemDelete");
    if (!original_copy || !original_add || !original_update || !original_delete) _exit(78);
}
OSStatus SecItemCopyMatching(CFDictionaryRef q, CFTypeRef *result) {
    pthread_once(&originals_once, load_originals);
    if (!chromium_item(q)) return original_copy(q, result);
    if (!ready) return errSecInteractionNotAllowed;
    CFMutableDictionaryRef copy = own_query(q);
    OSStatus status = original_copy(copy, result);
    CFRelease(copy);
    record("cef-own-key-read", status);
    require_access(status);
    return status;
}
OSStatus SecItemAdd(CFDictionaryRef q, CFTypeRef *result) {
    pthread_once(&originals_once, load_originals);
    if (!chromium_item(q)) return original_add(q, result);
    // Startup creates the key once. Chromium must never replace it.
    record("cef-key-create-rejected", errSecDuplicateItem);
    return errSecDuplicateItem;
}
OSStatus SecItemUpdate(CFDictionaryRef q, CFDictionaryRef attributes) {
    pthread_once(&originals_once, load_originals);
    if (!chromium_item(q)) return original_update(q, attributes);
    record("cef-key-update-rejected", errSecAuthFailed);
    return errSecAuthFailed;
}
OSStatus SecItemDelete(CFDictionaryRef q) {
    pthread_once(&originals_once, load_originals);
    if (!chromium_item(q)) return original_delete(q);
    record("cef-key-delete-rejected", errSecAuthFailed);
    return errSecAuthFailed;
}
int muniment_keychain_prepare(const char *audit_path, bool allow_create) {
    // This API governs this process, not the user's Keychain configuration.
    // It returns an error instead of showing an authorization or unlock dialog.
    OSStatus status = SecKeychainSetUserInteractionAllowed(false);
    if (status != errSecSuccess) return status;
    audit_log = fopen(audit_path, "a");
    if (!audit_log) return errSecIO;
    Boolean interaction = true;
    status = SecKeychainGetUserInteractionAllowed(&interaction);
    if (status != errSecSuccess || interaction) return errSecInteractionNotAllowed;
    record("noninteractive", 0);
    CFMutableDictionaryRef q = CFDictionaryCreateMutable(NULL, 0,
        &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFDictionarySetValue(q, kSecClass, kSecClassGenericPassword);
    CFDictionarySetValue(q, kSecAttrService, service());
    CFDictionarySetValue(q, kSecAttrAccount, account());
    CFDictionarySetValue(q, kSecReturnData, kCFBooleanTrue);
    CFTypeRef data = NULL;
    status = SecItemCopyMatching(q, &data);
    if (status == errSecSuccess && (!data || CFGetTypeID(data) != CFDataGetTypeID()
        || CFDataGetLength((CFDataRef)data) != 64)) status = errSecDecode;
    if (data) CFRelease(data);
    if (status == errSecItemNotFound && allow_create) {
        unsigned char random[32];
        char hex[64];
        status = SecRandomCopyBytes(kSecRandomDefault, sizeof(random), random);
        if (status == errSecSuccess) {
            const char digits[] = "0123456789abcdef";
            for (unsigned i = 0; i < sizeof(random); ++i) {
                hex[2*i] = digits[random[i] >> 4];
                hex[2*i+1] = digits[random[i] & 15];
            }
            CFDataRef key = CFDataCreate(NULL, (const UInt8 *)hex, sizeof(hex));
            CFDictionaryRemoveValue(q, kSecReturnData);
            CFDictionarySetValue(q, kSecValueData, key);
            status = SecItemAdd(q, NULL);
            CFRelease(key);
            memset_s(random, sizeof(random), 0, sizeof(random));
            memset_s(hex, sizeof(hex), 0, sizeof(hex));
            record("own-key-create", status);
        }
    }
    CFRelease(q);
    record("own-key-preflight", status);
    ready = status == errSecSuccess;
    return status;
}
