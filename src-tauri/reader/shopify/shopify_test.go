package shopify

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeShopify answers the shop, the three lists and their counts from
// fixtures, pages through a Link header whose page_info is an index, and
// refuses a wrong token.
func fakeShopify(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	customers := []map[string]any{
		{"id": 101.0, "first_name": "Ann", "last_name": "Lee", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000", "state": "enabled", "verified_email": true, "tax_exempt": false,
			"orders_count": 3.0, "total_spent": "157.00", "currency": "USD", "note": "VIP", "tags": "wholesale, vip",
			"default_address":         map[string]any{"company": "Northwind Traders", "city": "Seattle", "province": "Washington", "country_code": "US", "zip": "98101"},
			"email_marketing_consent": map[string]any{"state": "subscribed"},
			"created_at":              "2023-11-14T14:13:20-08:00", "updated_at": "2023-11-16T01:00:00-08:00"},
		{"id": 102.0, "first_name": "Bo", "email": nil, "state": "disabled", "orders_count": 0.0, "total_spent": "0.00", "currency": "USD", "tags": "", "created_at": "2023-11-14T22:15:00Z", "updated_at": "2023-11-15T09:00:00Z"},
		{"id": 103.0, "last_name": "Fabrikam", "email": "ops@fabrikam.example", "state": "invited", "orders_count": 1.0, "total_spent": "20.00", "currency": "EUR", "created_at": "2023-11-13T22:13:20Z", "updated_at": "2023-11-14T09:00:00Z"},
	}
	orders := []map[string]any{
		{"id": 1001.0, "name": "#1001", "order_number": 1001.0, "email": "ann@northwind.example", "customer": map[string]any{"id": 101.0, "first_name": "Ann", "last_name": "Lee"},
			"financial_status": "paid", "fulfillment_status": "fulfilled", "total_price": "157.00", "subtotal_price": "145.00", "total_tax": "12.00", "total_discounts": "0.00", "currency": "USD",
			"line_items": []any{map[string]any{"title": "Tea"}, map[string]any{"title": "Cup"}}, "source_name": "web", "tags": "rush", "test": false,
			"created_at": "2023-11-14T22:20:00Z", "processed_at": "2023-11-14T22:20:00Z", "closed_at": "2023-11-14T23:00:00Z", "cancelled_at": nil, "updated_at": "2023-11-14T23:00:00Z"},
		{"id": 1002.0, "name": "#1002", "order_number": 1002.0, "financial_status": "voided", "total_price": "5000", "currency": "JPY", "cancel_reason": "customer",
			"created_at": "2023-11-15T22:20:00Z", "cancelled_at": "2023-11-15T22:30:00Z", "updated_at": "2023-11-15T22:30:00Z"},
	}
	products := []map[string]any{
		{"id": 501.0, "title": "Green Tea", "handle": "green-tea", "vendor": "Northwind", "product_type": "Tea", "status": "active", "tags": "organic, tea",
			"variants":     []any{map[string]any{"price": "12.50", "sku": "TEA-1", "inventory_quantity": 4.0}, map[string]any{"price": "22.00", "sku": "TEA-2", "inventory_quantity": 6.0}},
			"published_at": "2023-11-01T00:00:00Z", "created_at": "2023-10-01T00:00:00Z", "updated_at": "2023-11-10T00:00:00Z"},
	}
	data := map[string][]map[string]any{"customers": customers, "orders": orders, "products": products}
	prefix := "/admin/api/" + APIVersion + "/"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		if r.Header.Get("X-Shopify-Access-Token") != "shpat_good" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"errors":"[API] Invalid API key or access token (unrecognized login or wrong password)"}`))
			return
		}
		if r.URL.Path == prefix+"shop.json" {
			_ = json.NewEncoder(w).Encode(map[string]any{"shop": map[string]any{"id": 1.0, "name": "Northwind", "myshopify_domain": "northwind.myshopify.com"}})
			return
		}
		resource := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, prefix), ".json")
		counting := strings.HasSuffix(resource, "/count")
		resource = strings.TrimSuffix(resource, "/count")
		rows, ok := data[resource]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"errors":"Not Found"}`))
			return
		}
		if resource == "orders" && r.URL.Query().Get("page_info") == "" && r.URL.Query().Get("status") != "any" {
			t.Errorf("orders listed without status=any: %s", r.URL.RawQuery)
		}
		if resource == "customers" && r.URL.Query().Get("status") != "" {
			t.Errorf("customers listed with an order filter: %s", r.URL.RawQuery)
		}
		if since := r.URL.Query().Get("updated_at_min"); since != "" {
			kept := []map[string]any{}
			for _, row := range rows {
				if formatTime(row["updated_at"].(string)) >= since {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if counting {
			_ = json.NewEncoder(w).Encode(map[string]any{"count": len(rows)})
			return
		}
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		start := 0
		if info := r.URL.Query().Get("page_info"); info != "" {
			for key := range r.URL.Query() {
				if key != "limit" && key != "page_info" {
					w.WriteHeader(http.StatusBadRequest)
					_, _ = w.Write([]byte(`{"errors":"page_info - Invalid value. Only limit and fields are allowed with page_info."}`))
					return
				}
			}
			start, _ = strconv.Atoi(info)
		}
		end := start + limit
		if end > len(rows) {
			end = len(rows)
		}
		links := []string{}
		if start > 0 {
			links = append(links, fmt.Sprintf(`<%s%s?limit=%d&page_info=%d>; rel="previous"`, "https://northwind.myshopify.com", r.URL.Path, limit, 0))
		}
		if end < len(rows) {
			links = append(links, fmt.Sprintf(`<%s%s?limit=%d&page_info=%d>; rel="next"`, "https://northwind.myshopify.com", r.URL.Path, limit, end))
		}
		if len(links) > 0 {
			w.Header().Set("Link", strings.Join(links, ", "))
		}
		_ = json.NewEncoder(w).Encode(map[string]any{resource: rows[start:end]})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(token string) string {
	body, _ := json.Marshal(map[string]string{"shop": "northwind", "access_token": token})
	return string(body)
}

func TestObjectsProveTheTokenAndBuildTheHost(t *testing.T) {
	server, calls := fakeShopify(t)
	source := New(packed("shpat_good"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "customers" || objects[2].Label != "Products" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "/admin/api/"+APIVersion+"/shop.json") {
		t.Fatalf("the token was not proved with the shop call: %v", *calls)
	}
	if live := New(packed("x"), ""); live.baseURL != "https://northwind.myshopify.com" {
		t.Fatalf("the shop builds the host: %s", live.baseURL)
	}
	if live := New(`{"shop":"https://Northwind.myshopify.com/admin","access_token":"x"}`, ""); live.baseURL != "https://northwind.myshopify.com" {
		t.Fatalf("a pasted address reads as its shop: %s", live.baseURL)
	}
	_, err = New(packed("shpat_bad"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New(`{"shop":"","access_token":""}`, "").Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" {
		t.Fatalf("missing credentials must read as not_connected, got %v", err)
	}
	_, err = source.Describe("collections")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	_, err = New(packed("shpat_bad"), server.URL).Page("orders", nil, 0)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" {
		t.Fatalf("a refused page must read as not_connected, got %v", err)
	}
}

func TestDescribeCountsAndFlattensCustomers(t *testing.T) {
	server, calls := fakeShopify(t)
	description, err := New(packed("shpat_good"), server.URL).Describe("customers")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Customers" || description.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	if !strings.Contains((*calls)[1], "customers/count.json") {
		t.Fatalf("the count was not read: %v", *calls)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["total_spent"].Guess != "number" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["email_domain"].Samples; len(got) != 2 || got[0] != "northwind.example" {
		t.Fatalf("email domains: %v", got)
	}
	if byName["email"].Filled != 2 || byName["name"].Filled != 3 || byName["company"].Filled != 1 {
		t.Fatalf("filled counts: %+v", byName)
	}
	if got := byName["created"].Samples[0]; got != "2023-11-14T22:13:20Z" {
		t.Fatalf("created reads in UTC: %s", got)
	}
	if got := byName["tags"].Samples; got[0] != "vip,wholesale" {
		t.Fatalf("tags sort: %v", got)
	}
	if got := byName["verified_email"].Samples; got[0] != "yes" {
		t.Fatalf("booleans: %v", got)
	}
	if got := byName["marketing"].Samples; got[0] != "subscribed" {
		t.Fatalf("marketing: %v", got)
	}
}

func TestPageWalksTheLinkHeaderAndFlattensOrders(t *testing.T) {
	server, calls := fakeShopify(t)
	source := New(packed("shpat_good"), server.URL)
	first, err := source.Page("customers", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("customers", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "103" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("the mark keeps the newest update seen: %s", second.Hash)
	}
	if !strings.Contains((*calls)[1], "page_info=2") {
		t.Fatalf("the second page did not carry the page_info: %v", *calls)
	}

	orders, err := source.Page("orders", nil, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(orders.Rows) != 1 || orders.Next == nil || orders.Next.Token != "1" {
		t.Fatalf("orders: %+v", orders)
	}
	row := orders.Rows[0]
	if row["customer_id"] != "101" || row["customer_name"] != "Ann Lee" || row["status"] != "closed" || row["financial_status"] != "paid" {
		t.Fatalf("order row: %v", row)
	}
	if row["total"] != "157.00" || row["tax"] != "12.00" || row["currency"] != "USD" || row["line_items"] != "2" || row["test"] != "no" {
		t.Fatalf("order money: %v", row)
	}
	more, err := source.Page("orders", orders.Next, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(more.Rows) != 1 || more.Next != nil {
		t.Fatalf("the second order page: %+v", more)
	}
	if last := (*calls)[len(*calls)-1]; strings.Contains(last, "status=") {
		t.Fatalf("a page_info page must carry the limit alone: %s", last)
	}
	cancelled := more.Rows[0]
	if cancelled["status"] != "cancelled" || cancelled["cancel_reason"] != "customer" || cancelled["customer_id"] != "" || cancelled["cancelled_at"] != "2023-11-15T22:30:00Z" {
		t.Fatalf("a cancelled order: %v", cancelled)
	}

	products, err := source.Page("products", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	product := products.Rows[0]
	if product["title"] != "Green Tea" || product["price"] != "12.50" || product["sku"] != "TEA-1" || product["variants"] != "2" || product["inventory"] != "10" || product["tags"] != "organic,tea" {
		t.Fatalf("product row: %v", product)
	}
}

func TestDeltaFiltersByUpdatedAtMin(t *testing.T) {
	server, calls := fakeShopify(t)
	source := New(packed("shpat_good"), server.URL)
	delta, err := source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-16T09:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	if !strings.Contains((*calls)[0], "updated_at_min=2023-11-16T09%3A00%3A00Z") {
		t.Fatalf("the list did not filter by the mark: %v", *calls)
	}
	delta, err = source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-15T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("orders", nil)
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-15T22:30:00Z" {
		t.Fatalf("a first delta: %+v %v", delta, err)
	}
	if last := (*calls)[len(*calls)-1]; !strings.Contains(last, "status=any") || strings.Contains(last, "updated_at_min") {
		t.Fatalf("a first delta asks for every order: %s", last)
	}
}

func TestLinkHeadersAndMarks(t *testing.T) {
	header := `<https://northwind.myshopify.com/admin/api/2024-07/customers.json?limit=2&page_info=abc>; rel="previous", <https://northwind.myshopify.com/admin/api/2024-07/customers.json?limit=2&page_info=def>; rel="next"`
	if nextPageInfo(header) != "def" {
		t.Fatalf("the next page_info: %q", nextPageInfo(header))
	}
	if nextPageInfo(`<https://northwind.myshopify.com/x.json?page_info=abc>; rel="previous"`) != "" || nextPageInfo("") != "" {
		t.Fatal("a last page has no next page_info")
	}
	if shopName("https://Northwind.myshopify.com/") != "northwind" || shopName("northwind") != "northwind" {
		t.Fatal("the shop name did not read")
	}
	if newestUpdated(nil, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("an empty list keeps the mark")
	}
	if newestUpdated([]record{{"updated_at": "2022-01-01T00:00:00Z"}}, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
}
