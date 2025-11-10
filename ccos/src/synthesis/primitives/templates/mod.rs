use super::{
    annotation_string, annotation_string_map, annotation_string_vec, PrimitiveContext,
    PrimitiveTemplate, SynthesizedPrimitive,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

/// Identifier for primitive templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveTemplateId {
    Filter,
    Map,
    Project,
    Reduce,
    Sort,
    GroupBy,
    Join,
}

impl PrimitiveTemplateId {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrimitiveTemplateId::Filter => "filter",
            PrimitiveTemplateId::Map => "map",
            PrimitiveTemplateId::Project => "project",
            PrimitiveTemplateId::Reduce => "reduce",
            PrimitiveTemplateId::Sort => "sort",
            PrimitiveTemplateId::GroupBy => "groupBy",
            PrimitiveTemplateId::Join => "join",
        }
    }
}

impl std::fmt::Display for PrimitiveTemplateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Filter primitive template – generates RTFS that filters a collection based on a
/// search string across one or more fields.
#[derive(Debug, Default)]
pub struct FilterPrimitiveTemplate;

impl PrimitiveTemplate for FilterPrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::Filter
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| kind.eq_ignore_ascii_case("filter"))
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        // Allow matching by capability class containing ".filter" as a secondary signal.
        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains("filter"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let collection_input = annotation_string(ctx, &["primitive", "collection_input"])
            .or_else(|| ctx.need.required_inputs.first().cloned())
            .ok_or_else(|| anyhow!("filter primitive requires a collection input binding"))?;

        let search_input = annotation_string(ctx, &["primitive", "search_input"])
            .or_else(|| ctx.need.required_inputs.get(1).cloned())
            .ok_or_else(|| anyhow!("filter primitive requires a search input binding"))?;

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "filtered".to_string());

        let search_fields =
            annotation_string_vec(ctx, &["primitive", "search_fields"]).unwrap_or_default();

        let rtfs_code = build_filter_rtfs(
            &collection_input,
            &search_input,
            &output_key,
            &search_fields,
        );

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::Filter,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "filter",
                "collection_input": collection_input,
                "search_input": search_input,
                "output_key": output_key,
                "search_fields": search_fields,
            }),
        })
    }
}

/// Map primitive template – restructures each element according to a provided field mapping.
#[derive(Debug, Default)]
pub struct MapPrimitiveTemplate;

impl PrimitiveTemplate for MapPrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::Map
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| kind.eq_ignore_ascii_case("map"))
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains(".map"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let collection_input = annotation_string(ctx, &["primitive", "collection_input"])
            .or_else(|| ctx.need.required_inputs.first().cloned())
            .ok_or_else(|| anyhow!("map primitive requires a collection input binding"))?;

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "mapped".to_string());

        let mapping = annotation_string_map(ctx, &["primitive", "mapping"])
            .filter(|m| !m.is_empty())
            .ok_or_else(|| anyhow!("map primitive requires a non-empty mapping annotation"))?;

        let mapping_pairs: Vec<(String, String)> =
            mapping.into_iter().map(|(k, v)| (k, v)).collect();

        let rtfs_code =
            build_map_like_rtfs(&collection_input, &output_key, &mapping_pairs, "assoc");

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::Map,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "map",
                "collection_input": collection_input,
                "output_key": output_key,
                "mapping": mapping_pairs,
            }),
        })
    }
}

/// Project primitive template – keeps only the requested fields on each element.
#[derive(Debug, Default)]
pub struct ProjectPrimitiveTemplate;

impl PrimitiveTemplate for ProjectPrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::Project
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| kind.eq_ignore_ascii_case("project"))
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains("project") || class.contains("select"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let collection_input = annotation_string(ctx, &["primitive", "collection_input"])
            .or_else(|| ctx.need.required_inputs.first().cloned())
            .ok_or_else(|| anyhow!("project primitive requires a collection input binding"))?;

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "projected".to_string());

        let fields = annotation_string_vec(ctx, &["primitive", "fields"])
            .filter(|f| !f.is_empty())
            .ok_or_else(|| anyhow!("project primitive requires a non-empty fields list"))?;

        let mapping_pairs: Vec<(String, String)> = fields
            .iter()
            .map(|field| (field.clone(), field.clone()))
            .collect();

        let rtfs_code =
            build_map_like_rtfs(&collection_input, &output_key, &mapping_pairs, "assoc");

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::Project,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "project",
                "collection_input": collection_input,
                "output_key": output_key,
                "fields": fields,
            }),
        })
    }
}

/// Reduce primitive template – collapses a collection into a single value.
#[derive(Debug, Default)]
pub struct ReducePrimitiveTemplate;

impl PrimitiveTemplate for ReducePrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::Reduce
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| kind.eq_ignore_ascii_case("reduce"))
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains("reduce") || class.contains("aggregate"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let collection_input = annotation_string(ctx, &["primitive", "collection_input"])
            .or_else(|| ctx.need.required_inputs.first().cloned())
            .ok_or_else(|| anyhow!("reduce primitive requires a collection input binding"))?;

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "reduced".to_string());

        let reducer_config = ctx
            .annotation_path(&["primitive", "reducer"])
            .ok_or_else(|| anyhow!("reduce primitive requires a reducer configuration"))?;

        let reducer_fn = reducer_config
            .get("fn")
            .and_then(|v| v.as_str())
            .unwrap_or("+")
            .trim()
            .to_string();

        if reducer_fn.is_empty() {
            return Err(anyhow!("reduce primitive reducer fn cannot be empty"));
        }

        let item_field = reducer_config
            .get("item_field")
            .and_then(|v| v.as_str())
            .unwrap_or(":value")
            .to_string();

        let initial_literal = reducer_config
            .get("initial")
            .map(json_to_rtfs_literal)
            .transpose()?
            .unwrap_or_else(|| "0".to_string());

        let item_default_literal = reducer_config
            .get("item_default")
            .map(json_to_rtfs_literal)
            .transpose()?
            .unwrap_or_else(|| initial_literal.clone());

        let rtfs_code = build_reduce_rtfs(
            &collection_input,
            &output_key,
            &reducer_fn,
            &item_field,
            &initial_literal,
            &item_default_literal,
        );

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::Reduce,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "reduce",
                "collection_input": collection_input,
                "output_key": output_key,
                "reducer": {
                    "fn": reducer_fn,
                    "item_field": item_field,
                    "initial": initial_literal,
                    "item_default": item_default_literal,
                }
            }),
        })
    }
}

/// Sort primitive template – orders a collection by a key.
#[derive(Debug, Default)]
pub struct SortPrimitiveTemplate;

impl PrimitiveTemplate for SortPrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::Sort
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| kind.eq_ignore_ascii_case("sort"))
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains("sort") || class.contains("order"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let collection_input = annotation_string(ctx, &["primitive", "collection_input"])
            .or_else(|| ctx.need.required_inputs.first().cloned())
            .ok_or_else(|| anyhow!("sort primitive requires a collection input binding"))?;

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "sorted".to_string());

        let sort_key = annotation_string(ctx, &["primitive", "sort_key"])
            .ok_or_else(|| anyhow!("sort primitive requires a sort_key annotation"))?;

        let order =
            annotation_string(ctx, &["primitive", "order"]).unwrap_or_else(|| ":asc".to_string());

        let order_keyword = keyword_literal(&order);
        if order_keyword != ":asc" && order_keyword != ":desc" {
            return Err(anyhow!(
                "sort primitive order must be :asc or :desc, found '{}'",
                order
            ));
        }

        let rtfs_code = build_sort_rtfs(&collection_input, &output_key, &sort_key, &order_keyword);

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::Sort,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "sort",
                "collection_input": collection_input,
                "output_key": output_key,
                "sort_key": sort_key,
                "order": order_keyword,
            }),
        })
    }
}

/// GroupBy primitive template – partitions a collection by a key.
#[derive(Debug, Default)]
pub struct GroupByPrimitiveTemplate;

impl PrimitiveTemplate for GroupByPrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::GroupBy
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| {
                kind.eq_ignore_ascii_case("groupby") || kind.eq_ignore_ascii_case("group_by")
            })
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains("group") && class.contains("by"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let collection_input = annotation_string(ctx, &["primitive", "collection_input"])
            .or_else(|| ctx.need.required_inputs.first().cloned())
            .ok_or_else(|| anyhow!("groupBy primitive requires a collection input binding"))?;

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "grouped".to_string());

        let group_key = annotation_string(ctx, &["primitive", "group_key"])
            .ok_or_else(|| anyhow!("groupBy primitive requires a group_key annotation"))?;

        let rtfs_code = build_group_by_rtfs(&collection_input, &output_key, &group_key);

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::GroupBy,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "groupBy",
                "collection_input": collection_input,
                "output_key": output_key,
                "group_key": group_key,
            }),
        })
    }
}

/// Join primitive template – combines two collections on matching keys.
#[derive(Debug, Default)]
pub struct JoinPrimitiveTemplate;

impl PrimitiveTemplate for JoinPrimitiveTemplate {
    fn id(&self) -> PrimitiveTemplateId {
        PrimitiveTemplateId::Join
    }

    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool> {
        let annotated = annotation_string(ctx, &["primitive", "kind"])
            .map(|kind| kind.eq_ignore_ascii_case("join"))
            .unwrap_or(false);

        if annotated {
            return Ok(true);
        }

        let class = ctx.need.capability_class.to_lowercase();
        Ok(class.contains("join") || class.contains("merge"))
    }

    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        let required_inputs = &ctx.need.required_inputs;
        if required_inputs.len() < 2 {
            return Err(anyhow!(
                "join primitive requires at least two collection inputs (left and right)"
            ));
        }

        let left_input = annotation_string(ctx, &["primitive", "left_input"])
            .unwrap_or_else(|| required_inputs[0].clone());
        let right_input = annotation_string(ctx, &["primitive", "right_input"])
            .unwrap_or_else(|| required_inputs[1].clone());

        let output_key = annotation_string(ctx, &["primitive", "output_key"])
            .or_else(|| ctx.need.expected_outputs.first().cloned())
            .unwrap_or_else(|| "joined".to_string());

        let on = ctx
            .annotation_path(&["primitive", "on"])
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                anyhow!("join primitive requires an 'on' annotation with [left right]")
            })?;

        if on.len() != 2 {
            return Err(anyhow!(
                "join primitive 'on' annotation must contain exactly two entries"
            ));
        }

        let left_key = on[0]
            .as_str()
            .ok_or_else(|| anyhow!("join primitive left key must be a string"))?
            .to_string();
        let right_key = on[1]
            .as_str()
            .ok_or_else(|| anyhow!("join primitive right key must be a string"))?
            .to_string();

        let join_type =
            annotation_string(ctx, &["primitive", "type"]).unwrap_or_else(|| ":inner".to_string());

        let join_keyword = keyword_literal(&join_type);
        if join_keyword != ":inner" && join_keyword != ":left" {
            return Err(anyhow!(
                "join primitive currently supports :inner or :left joins, found '{}'",
                join_type
            ));
        }

        let rtfs_code = build_join_rtfs(
            &left_input,
            &right_input,
            &output_key,
            &left_key,
            &right_key,
            &join_keyword,
        );

        Ok(SynthesizedPrimitive {
            capability_id: ctx.need.capability_class.clone(),
            primitive_id: PrimitiveTemplateId::Join,
            rtfs_code,
            input_schema: ctx.aggregated_input_schema(),
            output_schema: ctx.aggregated_output_schema(),
            metadata: json!({
                "primitive": "join",
                "left_input": left_input,
                "right_input": right_input,
                "output_key": output_key,
                "on": [left_key, right_key],
                "type": join_keyword,
            }),
        })
    }
}

fn build_filter_rtfs(
    collection_input: &str,
    search_input: &str,
    output_key: &str,
    search_fields: &[String],
) -> String {
    let collection_kw = keyword_literal(collection_input);
    let search_kw = keyword_literal(search_input);
    let output_kw = keyword_literal(output_key);

    let mut field_let_lines = Vec::new();
    let mut predicate_lines = Vec::new();

    for (idx, field) in search_fields.iter().enumerate() {
        let binding_name = format!("field_{}", idx);
        field_let_lines.push(format!(
            "          {binding_name} (string-lower (str (get item {} \"\")))",
            keyword_literal(field)
        ));
        predicate_lines.push(format!(
            "              (string-contains {binding_name} search-str)"
        ));
    }

    let field_let_block = if field_let_lines.is_empty() {
        "          item-str (string-lower (str item))".to_string()
    } else {
        let mut lines = field_let_lines;
        lines.push("          item-str (string-lower (str item))".to_string());
        lines.join("\n")
    };

    let mut predicate_block = String::new();
    for line in &predicate_lines {
        predicate_block.push_str(line);
        predicate_block.push('\n');
    }
    predicate_block.push_str("              (string-contains item-str search-str)");

    format!(
        r#"(fn [input]
  (let [
    raw-items (get input {collection_kw})
    items (if (vector? raw-items)
            raw-items
            (if (map? raw-items)
              (let [
                direct (or
                  (get raw-items :items)
                  (get raw-items :results)
                  (get raw-items :data)
                  (get raw-items :edges)
                  (get raw-items :nodes)
                  (get raw-items :values)
                  (get raw-items :entries)
                  (get raw-items :content))
              ]
                (if (vector? direct) direct []))
              []))
    search-str (string-lower (str (get input {search_kw} "")))
    filtered-items (filter
      (fn [item]
        (let [
{field_let_block}
        ]
          (if (= search-str "")
            true
            (or
{predicate_block}
            ))))
      items)
  ]
    {{{output_kw} filtered-items}})
)"#
    )
}

fn keyword_literal(ident: &str) -> String {
    if ident.trim().starts_with(':') {
        ident.trim().to_string()
    } else {
        format!(":{}", ident.trim())
    }
}

fn build_map_like_rtfs(
    collection_input: &str,
    output_key: &str,
    mapping: &[(String, String)],
    assoc_fn: &str,
) -> String {
    let collection_kw = keyword_literal(collection_input);
    let output_kw = keyword_literal(output_key);

    let mut bindings = vec!["          acc {}".to_string()];
    for (out_key, src_key) in mapping {
        bindings.push(format!(
            "          acc ({assoc_fn} acc {} (get item {}))",
            keyword_literal(out_key),
            keyword_literal(src_key),
            assoc_fn = assoc_fn
        ));
    }

    let binding_block = bindings.join("\n");

    format!(
        r#"(fn [input]
  (let [
    items (or (get input {collection_kw}) [])
    mapped-items (map
      (fn [item]
        (let [
{binding_block}
        ]
          acc))
      items)
  ]
    {{{output_kw} mapped-items}})
)"#
    )
}

fn build_reduce_rtfs(
    collection_input: &str,
    output_key: &str,
    reducer_fn: &str,
    item_field: &str,
    initial_literal: &str,
    item_default_literal: &str,
) -> String {
    let collection_kw = keyword_literal(collection_input);
    let output_kw = keyword_literal(output_key);
    let item_field_kw = keyword_literal(item_field);

    format!(
        r#"(fn [input]
  (let [
    items (or (get input {collection_kw}) [])
    reduced (reduce
      (fn [acc item]
        ({reducer_fn} acc (get item {item_field_kw} {item_default_literal})))
      {initial_literal}
      items)
  ]
    {{{output_kw} reduced}})
)"#
    )
}

fn build_sort_rtfs(
    collection_input: &str,
    output_key: &str,
    sort_key: &str,
    order_kw: &str,
) -> String {
    let collection_kw = keyword_literal(collection_input);
    let output_kw = keyword_literal(output_key);
    let sort_key_kw = keyword_literal(sort_key);

    format!(
        r#"(fn [input]
  (let [
    items (or (get input {collection_kw}) [])
    sorted-items (sort-by (fn [item] (get item {sort_key_kw})) items)
    final-items (if (= {order_kw} :desc)
      (reverse sorted-items)
      sorted-items)
  ]
    {{{output_kw} final-items}})
)"#
    )
}

fn build_group_by_rtfs(collection_input: &str, output_key: &str, group_key: &str) -> String {
    let collection_kw = keyword_literal(collection_input);
    let output_kw = keyword_literal(output_key);
    let group_key_kw = keyword_literal(group_key);

    format!(
        r#"(fn [input]
  (let [
    items (or (get input {collection_kw}) [])
    grouped (reduce
      (fn [acc item]
        (let [
          key (get item {group_key_kw})
          bucket (get acc key [])
        ]
          (assoc acc key (conj bucket item))))
      {{}}
      items)
  ]
    {{{output_kw} grouped}})
)"#
    )
}

fn build_join_rtfs(
    left_input: &str,
    right_input: &str,
    output_key: &str,
    left_key: &str,
    right_key: &str,
    join_type_kw: &str,
) -> String {
    let left_kw = keyword_literal(left_input);
    let right_kw = keyword_literal(right_input);
    let output_kw = keyword_literal(output_key);
    let left_key_kw = keyword_literal(left_key);
    let right_key_kw = keyword_literal(right_key);

    format!(
        r#"(fn [input]
  (let [
    left-items (or (get input {left_kw}) [])
    right-items (or (get input {right_kw}) [])
    join-type {join_type_kw}
    joined (reduce
      (fn [acc left-item]
        (let [
          left-key (get left-item {left_key_kw})
          matches (filter (fn [right-item] (= left-key (get right-item {right_key_kw}))) right-items)
        ]
          (if (empty? matches)
            (if (= join-type :left)
              (conj acc left-item)
              acc)
            (reduce
              (fn [inner-acc right-item]
                (conj inner-acc (merge left-item right-item)))
              acc
              matches))))
      []
      left-items)
  ]
    {{{output_kw} joined}})
)"#
    )
}

fn json_to_rtfs_literal(value: &JsonValue) -> Result<String> {
    match value {
        JsonValue::Null => Ok("nil".to_string()),
        JsonValue::Bool(b) => Ok(if *b {
            "true".to_string()
        } else {
            "false".to_string()
        }),
        JsonValue::Number(n) => Ok(n.to_string()),
        JsonValue::String(s) => {
            if s.trim_start().starts_with(':') {
                Ok(keyword_literal(s))
            } else {
                Ok(format!("\"{}\"", escape_string(s)))
            }
        }
        JsonValue::Array(values) => {
            let mut parts = Vec::new();
            for v in values {
                parts.push(json_to_rtfs_literal(v)?);
            }
            Ok(format!("[{}]", parts.join(" ")))
        }
        JsonValue::Object(_) => Err(anyhow!(
            "object literals are not supported in primitive annotations"
        )),
    }
}

fn escape_string(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}
