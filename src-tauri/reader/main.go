// muniment-reader <source>: the network reader sidecar. It reads one JSON
// request on stdin, answers one JSON body on stdout and exits. The runtime
// spawns it per call, holds every cursor and does every write.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"muniment.ai/reader/airtable"
	"muniment.ai/reader/contract"
	"muniment.ai/reader/freshbooks"
	"muniment.ai/reader/freshdesk"
	"muniment.ai/reader/hubspot"
	"muniment.ai/reader/intercom"
	"muniment.ai/reader/notion"
	"muniment.ai/reader/outreach"
	"muniment.ai/reader/paypal"
	"muniment.ai/reader/pipedrive"
	"muniment.ai/reader/quickbooks"
	"muniment.ai/reader/salesforce"
	"muniment.ai/reader/salesloft"
	"muniment.ai/reader/sheets"
	"muniment.ai/reader/shopify"
	"muniment.ai/reader/square"
	"muniment.ai/reader/stripe"
	"muniment.ai/reader/wave"
	"muniment.ai/reader/zendesk"
	"muniment.ai/reader/zoho"
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
	case "zoho":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Zoho CRM with its client id, client secret, refresh token and accounts domain first.")
		}
		return zoho.New(secret, os.Getenv("MUNIMENT_ZOHO_BASE_URL")), nil
	case "outreach":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Outreach with its client id, client secret and refresh token first.")
		}
		return outreach.New(secret, os.Getenv("MUNIMENT_OUTREACH_BASE_URL")), nil
	case "salesloft":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Salesloft with its API key first.")
		}
		return salesloft.New(secret, os.Getenv("MUNIMENT_SALESLOFT_BASE_URL")), nil
	case "notion":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Notion with its internal integration token first.")
		}
		return notion.New(secret, os.Getenv("MUNIMENT_NOTION_BASE_URL")), nil
	case "airtable":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Airtable with its personal access token and base id first.")
		}
		return airtable.New(secret, os.Getenv("MUNIMENT_AIRTABLE_BASE_URL")), nil
	case "sheets":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Google Sheets with a service account key and the spreadsheet id first.")
		}
		return sheets.New(secret, os.Getenv("MUNIMENT_SHEETS_BASE_URL")), nil
	case "square":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Square with its access token first.")
		}
		return square.New(secret, os.Getenv("MUNIMENT_SQUARE_BASE_URL")), nil
	case "shopify":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Shopify with its myshopify domain and Admin API access token first.")
		}
		return shopify.New(secret, os.Getenv("MUNIMENT_SHOPIFY_BASE_URL")), nil
	case "paypal":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect PayPal with its client id and client secret first.")
		}
		return paypal.New(secret, os.Getenv("MUNIMENT_PAYPAL_BASE_URL")), nil
	case "freshbooks":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect FreshBooks with its client id, client secret, refresh token and account id first.")
		}
		return freshbooks.New(secret, os.Getenv("MUNIMENT_FRESHBOOKS_BASE_URL")), nil
	case "quickbooks":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect QuickBooks with its client id, client secret, refresh token and realm id first.")
		}
		return quickbooks.New(secret, os.Getenv("MUNIMENT_QUICKBOOKS_BASE_URL")), nil
	case "wave":
		secret := os.Getenv(secretVariable)
		if secret == "" {
			return nil, fail("not_connected", "Connect Wave with its full access token and business id first.")
		}
		return wave.New(secret, os.Getenv("MUNIMENT_WAVE_BASE_URL")), nil
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
