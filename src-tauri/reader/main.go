// muniment-reader <source>: the network reader sidecar. It reads one JSON
// request on stdin, answers one JSON body on stdout and exits. The runtime
// spawns it per call, holds every cursor and does every write.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"muniment.ai/reader/contract"
	"muniment.ai/reader/stripe"
)

// secretVariable carries the source's credential from the runtime.
const secretVariable = "MUNIMENT_READER_SECRET"

func openSource(name string) (Source, *contract.Failure) {
	switch name {
	case "stripe":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Stripe with its secret key first.")
		}
		return stripe.New(secret, os.Getenv("MUNIMENT_STRIPE_BASE_URL")), nil
	default:
		return nil, fail("unknown_source", "No reader reads %s.", name)
	}
}

func run(args []string, stdin io.Reader, stdout io.Writer) int {
	if len(args) != 1 {
		fmt.Fprintln(os.Stderr, "usage: muniment-reader <source>")
		return 2
	}
	var request contract.Request
	if err := json.NewDecoder(stdin).Decode(&request); err != nil {
		body, _ := json.Marshal(map[string]any{"error": fail("invalid_request", "the request does not parse: %s", err)})
		stdout.Write(body)
		return 1
	}
	source, failure := openSource(args[0])
	if failure != nil {
		body, _ := json.Marshal(map[string]any{"error": failure})
		stdout.Write(body)
		return 1
	}
	body, err := answer(source, request)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	stdout.Write(body)
	return 0
}

func main() {
	os.Exit(run(os.Args[1:], os.Stdin, os.Stdout))
}
