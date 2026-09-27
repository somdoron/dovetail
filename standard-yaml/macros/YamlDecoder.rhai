// Emits structural decoding only; runtime validation and diagnostics live in standard.yaml.
fn join(items, separator) {
    let result = "";
    for item in items {
        if result != "" { result += separator; }
        result += item;
    }
    result
}

fn quoted(name) { "\"" + name + "\"" }
fn allowed(fields, tag) {
    let names = [];
    if tag { names.push("\"type\""); }
    for field in fields { names.push(quoted(field.name)); }
    "[" + join(names, ", ") + "]"
}
fn recordBody(fields, constructor, indent, tag) {
    let source = indent + "try yaml.checkFields(" + allowed(fields, tag) + ")\n";
    let inits = [];
    let index = 0;
    for field in fields {
        let local = "__yamlField" + index;
        source += indent + "let " + local + " = try yaml.field<" + field.ty + ">(" + quoted(field.name) + ")\n";
        inits.push(field.name + " = " + local);
        index += 1;
    }
    source + indent + "Ok(" + constructor + " { " + join(inits, "; ") + " })"
}

let parameters = "";
let arguments = "";
let bounds = "";
if !input.type_params.is_empty() {
    arguments = "<" + join(input.type_params, ", ") + ">";
    parameters = arguments + " ";
    let requirements = [];
    for parameter in input.type_params { requirements.push(parameter + ": YamlDecoder"); }
    bounds = " where " + join(requirements, ", ");
}
let target = input.name + arguments;
let body = "";
let extra = "";
if input.kind == "newtype" {
    body = "        Ok(" + input.name + "(try " + input.inner + ".fromYaml(yaml)))";
    extra = "\n    public function fromMissing(): Result<" + target + ", YamlError> =\n        Ok(" + input.name + "(try " + input.inner + ".fromMissing()))\n";
} else if input.kind == "record" {
    body = recordBody(input.fields, input.name, "        ", false);
} else {
    if input.variants.is_empty() { throw "YamlDecoder cannot decode an empty enum"; }
    body = "        let __yamlTag = try yaml.field<String>(\"type\")\n";
    let first = true;
    for variant in input.variants {
        body += "        " + (if first { "if" } else { "else if" }) + " __yamlTag == " + quoted(variant.name) + " then\n";
        first = false;
        let constructor = input.name + "." + variant.name;
        if variant.kind == "none" {
            body += "            try yaml.checkFields([\"type\"])\n            Ok(" + constructor + ")\n";
        } else if variant.kind == "record" {
            for field in variant.fields {
                if field.name == "type" { throw "YamlDecoder: enum payload field 'type' conflicts with the discriminator"; }
            }
            body += recordBody(variant.fields, constructor, "            ", true) + "\n";
        } else {
            body += "            try yaml.checkFields([\"type\", \"value\"])\n";
            if variant.types.len() == 1 {
                body += "            let __yamlValue = try yaml.field<" + variant.types[0] + ">(\"value\")\n            Ok(" + constructor + "(__yamlValue))\n";
            } else {
                // Helpers retain payload indices and source spans for every enum arity.
                let values = [];
                body += "            let __yamlValues = try yaml.variantValues(" + variant.types.len() + ")\n";
                let index = 0;
                for ty in variant.types {
                    let local = "__yamlValue" + index;
                    body += "            let " + local + " = try yaml.variantElement<" + ty + ">(__yamlValues.get(" + index + "), " + index + ")\n";
                    values.push(local);
                    index += 1;
                }
                body += "            Ok(" + constructor + "(" + join(values, ", ") + "))\n";
            }
        }
    }
    body += "        else Error(yaml.unknownVariant(__yamlTag))";
}
"implement " + parameters + "YamlDecoder for " + target + bounds + " =\n    public function fromYaml(yaml: Yaml): Result<" + target + ", YamlError> =\n" + body + "\n" + extra
