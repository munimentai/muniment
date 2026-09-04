# Local mode

The desktop can open the thread surface without a Muniment account. Select **Use local mode** from the signed-out screen.

Local mode does not contact the Muniment control plane for authentication or model access. Pi runs on the device and reads its provider credentials from Pi's credential store. The desktop does not pass a Muniment virtual key to Pi in local mode.

## Provider access

Open the profile panel from the thread surface. Enter a Google API key under **Pi provider**, then select **Save Google key**. The desktop writes the key to Pi's `~/.pi/agent/auth.json` store with Pi's file lock.

Pi can also use provider credentials that the Pi CLI saved in the same store. Local mode removes inherited provider credential and endpoint variables from the Pi process environment.

## Local records

Local mode writes run input, model output, tool activity, and completion records to the local run journal. These records use the same event shapes as cloud-backed runs.

Select **Sign in for cloud features** to leave local mode. Sign-in remains available for features that need the Muniment control plane.
