use std::collections::BTreeMap;
use std::collections::BTreeSet;

use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, Sections,
};
use gimli::{Encoding, Format, LineEncoding};

use crate::common::types::MangledName;
use crate::typechecker::types::TypedFunction;

use super::FunctionDebugInfo;

/// Emit line program rows for a function's source mappings.
/// Only emits a sequence if there are valid (non-empty file) mappings.
fn emit_line_rows(
    program: &mut LineProgram,
    code_offset: u32,
    info: &FunctionDebugInfo,
    file_index_map: &BTreeMap<String, gimli::write::FileId>,
) {
    // Pre-filter to only include mappings with known files
    let valid_mappings: Vec<_> = info
        .mappings
        .iter()
        .filter(|m| {
            let file_key = m.span.file.as_ref();
            !file_key.is_empty() && file_index_map.contains_key(file_key)
        })
        .collect();

    if valid_mappings.is_empty() {
        return;
    }

    program.begin_sequence(Some(Address::Constant(code_offset as u64)));

    for mapping in &valid_mappings {
        let file_id = file_index_map[mapping.span.file.as_ref()];
        let row = program.row();
        row.address_offset = mapping.byte_offset as u64;
        row.file = file_id;
        row.line = mapping.span.line as u64;
        row.column = mapping.span.column as u64;
        row.is_statement = true;
        program.generate_row();
    }

    program.end_sequence(info.body_byte_len as u64);
}

/// Generate DWARF debug sections for embedding as WASM custom sections.
///
/// Returns a list of (section_name, section_bytes) pairs.
pub(super) fn generate_dwarf_sections(
    debug_infos: &[(u32, FunctionDebugInfo)],
    functions: &BTreeMap<MangledName, TypedFunction>,
    function_indices: &BTreeMap<MangledName, u32>,
) -> Vec<(String, Vec<u8>)> {
    if debug_infos.is_empty() {
        return Vec::new();
    }

    let encoding = Encoding {
        format: Format::Dwarf32,
        version: 4,
        address_size: 4,
    };

    let line_encoding = LineEncoding::default();

    // Collect unique source files from all mappings (skip synthetic empty paths)
    let mut file_set: BTreeSet<String> = BTreeSet::new();
    for (_, info) in debug_infos {
        for mapping in &info.mappings {
            let file_str = mapping.span.file.as_ref();
            if !file_str.is_empty() {
                file_set.insert(file_str.to_string());
            }
        }
    }

    if file_set.is_empty() {
        return Vec::new();
    }

    // Build line program with file table
    let comp_dir = LineString::String(b".".to_vec());
    let comp_file = if let Some(first_file) = file_set.iter().next() {
        LineString::String(first_file.as_bytes().to_vec())
    } else {
        LineString::String(b"<unknown>".to_vec())
    };

    let mut line_program = LineProgram::new(encoding, line_encoding, comp_dir, comp_file, None);

    // Add all source files and build a lookup map
    let mut file_index_map: BTreeMap<String, gimli::write::FileId> = BTreeMap::new();
    let default_dir = line_program.default_directory();
    for file_path in &file_set {
        let file_name = LineString::String(file_path.as_bytes().to_vec());
        let file_id = line_program.add_file(file_name, default_dir, None);
        file_index_map.insert(file_path.clone(), file_id);
    }

    // Create DWARF unit
    let mut dwarf = DwarfUnit::new(encoding);
    dwarf.unit.line_program = line_program;

    // Set compilation unit attributes
    let root_id = dwarf.unit.root();
    {
        let root = dwarf.unit.get_mut(root_id);
        root.set(
            gimli::DW_AT_producer,
            AttributeValue::String(b"Dovetail Compiler".to_vec()),
        );
        root.set(
            gimli::DW_AT_language,
            AttributeValue::Language(gimli::DW_LANG_Rust),
        );
        root.set(
            gimli::DW_AT_name,
            AttributeValue::String(b"dovetail".to_vec()),
        );
        root.set(
            gimli::DW_AT_comp_dir,
            AttributeValue::String(b".".to_vec()),
        );
        // low_pc = 0 (start of code section)
        root.set(
            gimli::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(0)),
        );
    }

    // Build ordered list of (mangled_name, function_index) for user functions
    let mut func_order: Vec<(&MangledName, u32)> = function_indices
        .iter()
        .map(|(name, &idx)| (name, idx))
        .collect();
    func_order.sort_by_key(|&(_, idx)| idx);

    // Add DW_TAG_subprogram entries for each user function with debug info
    // debug_infos are in the same order as functions in the typed module
    for (func_idx, (mangled, _)) in func_order.iter().enumerate() {
        if func_idx >= debug_infos.len() {
            break;
        }
        let (code_offset, ref info) = debug_infos[func_idx];

        let display_name = functions
            .get(*mangled)
            .map(|f| f.display_name.as_str())
            .unwrap_or(&mangled.0);

        let subprogram_id = dwarf.unit.add(root_id, gimli::DW_TAG_subprogram);
        let subprogram = dwarf.unit.get_mut(subprogram_id);
        subprogram.set(
            gimli::DW_AT_name,
            AttributeValue::String(display_name.as_bytes().to_vec()),
        );
        subprogram.set(
            gimli::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(code_offset as u64)),
        );
        subprogram.set(
            gimli::DW_AT_high_pc,
            AttributeValue::Udata(info.body_byte_len as u64),
        );

        emit_line_rows(
            &mut dwarf.unit.line_program,
            code_offset,
            info,
            &file_index_map,
        );
    }

    // Handle closure debug info (entries beyond user function count)
    for (i, &(code_offset, ref info)) in debug_infos.iter().enumerate().skip(func_order.len()) {
        let closure_name = format!("closure#{}", i - func_order.len());
        let subprogram_id = dwarf.unit.add(root_id, gimli::DW_TAG_subprogram);
        let subprogram = dwarf.unit.get_mut(subprogram_id);
        subprogram.set(
            gimli::DW_AT_name,
            AttributeValue::String(closure_name.as_bytes().to_vec()),
        );
        subprogram.set(
            gimli::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(code_offset as u64)),
        );
        subprogram.set(
            gimli::DW_AT_high_pc,
            AttributeValue::Udata(info.body_byte_len as u64),
        );

        emit_line_rows(
            &mut dwarf.unit.line_program,
            code_offset,
            info,
            &file_index_map,
        );
    }

    // Encode DWARF sections
    let mut sections = Sections::new(EndianVec::new(gimli::LittleEndian));
    dwarf.write(&mut sections).expect("DWARF encoding failed");

    // Collect non-empty sections
    let mut result = Vec::new();
    sections
        .for_each(|id, data| {
            let bytes = data.slice();
            if !bytes.is_empty() {
                let name = id.name().to_string();
                result.push((name, bytes.to_vec()));
            }
            Ok::<_, ()>(())
        })
        .unwrap();

    result
}
