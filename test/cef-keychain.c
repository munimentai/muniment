#include "../src-tauri/src/keychain_macos.c"
#include <assert.h>

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "unavailable") == 0) {
        // Run as a different, ad-hoc-signed executable after the real app creates
        // its item. This must fail without prompting or creating a replacement.
        int status = muniment_keychain_prepare(argv[2], false);
        assert(status == errSecInteractionNotAllowed || status == errSecAuthFailed);
        puts("PASS: unauthorized signer fails without a Keychain dialog");
        return 0;
    }
    CFMutableDictionaryRef q = CFDictionaryCreateMutable(NULL, 0,
        &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFDictionarySetValue(q, kSecClass, kSecClassGenericPassword);
    CFDictionarySetValue(q, kSecAttrService, CFSTR("Chromium Safe Storage"));
    CFDictionarySetValue(q, kSecAttrAccount, CFSTR("Chromium"));
    CFDictionarySetValue(q, kSecReturnData, kCFBooleanTrue);
    assert(chromium_item(q));
    CFMutableDictionaryRef copy = own_query(q);
    assert(equals(copy, kSecAttrService, service()));
    assert(equals(copy, kSecAttrAccount, account()));
    assert(equals(copy, kSecReturnData, kCFBooleanTrue));
    assert(chromium_item(q)); // Never mutate the caller's query.
    assert(!chromium_item(copy));
    CFDictionarySetValue(q, kSecAttrAccount, CFSTR("other"));
    assert(!chromium_item(q));
    CFDictionarySetValue(q, kSecAttrAccount, CFSTR("Chromium"));
    CFDictionarySetValue(q, kSecClass, kSecClassInternetPassword);
    assert(!chromium_item(q));
    CFDictionarySetValue(q, kSecClass, kSecClassGenericPassword);
    CFDictionarySetValue(q, kSecAttrService, CFSTR("Chrome Safe Storage"));
    assert(!chromium_item(q));
    assert(!chromium_item(NULL));
    CFRelease(copy);
    CFRelease(q);
    puts("PASS: exact key matching, isolated query copy, unrelated keys unchanged");
    return 0;
}
