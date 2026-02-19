// Resource regeneration: renewable resources grow back over time
// Note: resource mutations use "resource_name.field" format

for r in tile.resources {
    if r.renewal_rate > 0.0 && r.quantity < r.max_quantity {
        let new_qty = r.quantity + r.renewal_rate;
        if new_qty > r.max_quantity {
            set(r.resource_type + ".quantity", r.max_quantity);
        } else {
            set(r.resource_type + ".quantity", new_qty);
        }
    }
}
