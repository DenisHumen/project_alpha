// SPDX-License-Identifier: MPL-2.0
//
// Runtime check for a libsmb2 build (NET-002): the library loads, a context
// takes the options pa-smb2 sets, and a connection to a closed port fails
// cleanly with an error message. Exit code 0 = OK.
//
// Build: cc check_smb2.c -I<prefix>/include -L<prefix>/lib -lsmb2

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h> // the public headers use time_t without including it

#include <smb2/smb2.h>
#include <smb2/libsmb2.h>

int main(void) {
    struct smb2_context *smb2 = smb2_init_context();
    if (smb2 == NULL) {
        fprintf(stderr, "smb2_init_context failed\n");
        return 1;
    }
    smb2_set_version(smb2, SMB2_VERSION_ANY);
    smb2_set_security_mode(smb2, SMB2_NEGOTIATE_SIGNING_ENABLED);
    smb2_set_seal(smb2, 1);
    smb2_set_user(smb2, "guest");
    smb2_set_password(smb2, "");
    smb2_set_timeout(smb2, 2);

    // Port 1 on loopback is closed on CI machines: the connect must fail
    // (not hang, not crash) and leave an error message.
    int rc = smb2_connect_share(smb2, "127.0.0.1:1", "share", "guest");
    const char *err = smb2_get_error(smb2);
    if (rc >= 0) {
        fprintf(stderr, "connect to a closed port succeeded\n");
        smb2_destroy_context(smb2);
        return 1;
    }
    if (err == NULL || err[0] == '\0') {
        fprintf(stderr, "no error message after a failed connect (rc=%d)\n", rc);
        smb2_destroy_context(smb2);
        return 1;
    }
    printf("libsmb2 ok: connect to closed port -> %d (%s)\n", rc, err);
    smb2_destroy_context(smb2);
    return 0;
}
