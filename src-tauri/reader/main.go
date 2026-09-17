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
	"muniment.ai/reader/freshdesk"
	"muniment.ai/reader/hubspot"
	"muniment.ai/reader/intercom"
	"muniment.ai/reader/pipedrive"
	"muniment.ai/reader/salesforce"
	"muniment.ai/reader/stripe"
	"muniment.ai/reader/zendesk"
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
	case "hubspot":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect HubSpot with its private app access token first.")
		}
		return hubspot.New(secret, os.Getenv("MUNIMENT_HUBSPOT_BASE_URL")), nil
	case "pipedrive":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Pipedrive with its API token first.")
		}
		return pipedrive.New(secret, os.Getenv("MUNIMENT_PIPEDRIVE_BASE_URL")), nil
	case "salesforce":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Salesforce with its My Domain URL, consumer key and consumer secret first.")
		}
		return salesforce.New(secret), nil
	case "zendesk":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Zendesk with its subdomain, agent email and API token first.")
		}
		return zendesk.New(secret, os.Getenv("MUNIMENT_ZENDESK_BASE_URL")), nil
	case "intercom":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Intercom with its access token first.")
		}
		return intercom.New(secret, os.Getenv("MUNIMENT_INTERCOM_BASE_URL")), nil
	case "freshdesk":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Freshdesk with its domain and API key first.")
		}
		return freshdesk.New(secret, os.Getenv("MUNIMENT_FRESHDESK_BASE_URL")), nil
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
