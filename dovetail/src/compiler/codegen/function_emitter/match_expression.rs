use wasm_encoder::{BlockType, HeapType, Instruction};

use crate::typechecker::types::{Type, TypedExpr, TypedFieldPattern, TypedMatchArm, TypedPattern};

use super::{ExprContext, FunctionEmitter};

impl FunctionEmitter<'_> {
    /// Emit a match expression.
    pub(super) fn emit_match_expr(
        &mut self,
        expr: &TypedExpr,
        subject: &TypedExpr,
        arms: &[TypedMatchArm],
        ctx: ExprContext,
    ) {
        // Store every subject in its flattened locals, including transparent
        // newtypes over tuples or Uint128. Nested patterns consume the same layout.
        let scrutinee_valtypes = self.codegen.type_to_valtypes(&subject.ty);
        let scrutinee_local = self.add_value_locals(&scrutinee_valtypes);
        self.emit_expr(subject, ExprContext::Value);
        self.store_value(scrutinee_local, &scrutinee_valtypes);

        // 2. Wrap in an outer block for break-out after the matched arm. A tuple *result* can't be
        // an inline multi-value block result, so (like `if`-expressions) each arm spills its values
        // into result-temp locals via `emit_match_body` and we reload them after the block; the
        // block itself is empty. `match_result_spill` is saved/restored to survive nested matches.
        let result_is_tuple = ctx == ExprContext::Value
            && (self.codegen.is_tuple(&expr.ty) || self.codegen.is_uint128(&expr.ty));
        let prev_match_spill = self.match_result_spill.take();
        let block_type = if result_is_tuple {
            let valtypes = self.codegen.type_to_valtypes(&expr.ty);
            let base = self.add_result_temp(&valtypes);
            self.match_result_spill = Some((base, valtypes));
            BlockType::Empty
        } else if ctx == ExprContext::Value {
            BlockType::Result(self.codegen.single_val_type(&expr.ty))
        } else {
            BlockType::Empty
        };
        self.emit_block(block_type);
        let outer_block_depth = self.wasm_block_depth - 1;

        // 3. For each arm, emit test + body
        for arm in arms {
            match &arm.pattern {
                TypedPattern::Wildcard | TypedPattern::Variable(_, _) => {
                    // Wildcard/variable always matches
                    self.push_scope();
                    if let TypedPattern::Variable(name, _) = &arm.pattern {
                        // Bind the whole scrutinee (a flattened tuple aliases its run of locals).
                        self.bind_name(name.clone(), scrutinee_local);
                    }

                    if let Some(guard) = &arm.guard {
                        // Guard check
                        self.emit_expr(guard, ExprContext::Value);
                        self.emit_if_block(BlockType::Empty);
                        self.emit_match_body(&arm.body, ctx);
                        // br to outer block
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.emit_end_block(); // end if
                        self.pop_scope();
                    } else {
                        // Unconditional match — emit body and br
                        self.emit_match_body(&arm.body, ctx);
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.pop_scope();
                        break; // skip remaining arms
                    }
                }
                TypedPattern::Literal(lit_expr) => {
                    // Load scrutinee, emit literal, compare
                    self.instruction(Instruction::LocalGet(scrutinee_local));
                    self.emit_expr(lit_expr, ExprContext::Value);
                    self.emit_eq_instruction(&subject.ty);

                    if let Some(guard) = &arm.guard {
                        // Pattern match + guard: nested ifs
                        self.emit_if_block(BlockType::Empty);
                        self.emit_expr(guard, ExprContext::Value);
                        self.emit_if_block(BlockType::Empty);
                        self.push_scope();
                        self.emit_match_body(&arm.body, ctx);
                        self.pop_scope();
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.emit_end_block(); // end guard if
                        self.emit_end_block(); // end pattern if
                    } else {
                        self.emit_if_block(BlockType::Empty);
                        self.push_scope();
                        self.emit_match_body(&arm.body, ctx);
                        self.pop_scope();
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.emit_end_block(); // end if
                    }
                }
                TypedPattern::TypeAnnotated { binding, ty } => {
                    self.push_scope();
                    self.emit_block(BlockType::Empty);
                    let skip_depth = self.wasm_block_depth - 1;
                    self.emit_type_pattern(binding, ty, &subject.ty, scrutinee_local, skip_depth);
                    if let Some(guard) = &arm.guard {
                        self.emit_expr(guard, ExprContext::Value);
                        self.instruction(Instruction::I32Eqz);
                        let depth = self.wasm_block_depth - skip_depth - 1;
                        self.instruction(Instruction::BrIf(depth));
                    }
                    self.emit_match_body(&arm.body, ctx);
                    let depth = self.wasm_block_depth - outer_block_depth - 1;
                    self.instruction(Instruction::Br(depth));
                    self.emit_end_block();
                    self.pop_scope();
                }
                TypedPattern::Record { fields, .. } => {
                    let unconditional = self.emit_record_pattern_arm(
                        fields,
                        &arm.guard,
                        &arm.body,
                        &subject.ty,
                        scrutinee_local,
                        outer_block_depth,
                        ctx,
                    );
                    if unconditional {
                        break;
                    }
                }
                TypedPattern::EnumVariant {
                    enum_type,
                    variant_name,
                    payload_patterns,
                    ..
                } => {
                    self.emit_enum_variant_pattern_arm(
                        enum_type,
                        variant_name,
                        payload_patterns,
                        &arm.guard,
                        &arm.body,
                        scrutinee_local,
                        outer_block_depth,
                        ctx,
                    );
                    // Enum variant patterns are always conditional (ref.test)
                }
                TypedPattern::EnumVariantRecord {
                    enum_type,
                    variant_name,
                    field_patterns,
                    ..
                } => {
                    self.emit_enum_variant_record_pattern_arm(
                        enum_type,
                        variant_name,
                        field_patterns,
                        &arm.guard,
                        &arm.body,
                        scrutinee_local,
                        outer_block_depth,
                        ctx,
                    );
                }
                TypedPattern::Tuple {
                    element_patterns,
                    tuple_type,
                } => {
                    let tuple_elems: Vec<Type> = match tuple_type {
                        Type::Tuple(elems, _) => elems.clone(),
                        _ => unreachable!("Tuple pattern must have Tuple type"),
                    };

                    // Check if any element has a conditional sub-pattern
                    let has_conditional = element_patterns.iter().any(|p| {
                        !matches!(p, TypedPattern::Variable(_, _) | TypedPattern::Wildcard)
                    });

                    self.push_scope();

                    if has_conditional {
                        // Conditional: wrap in a block for early exit on mismatch
                        self.emit_block(BlockType::Empty);
                        let skip_depth = self.wasm_block_depth - 1;

                        for (field_index, sub_pat) in element_patterns.iter().enumerate() {
                            if matches!(sub_pat, TypedPattern::Wildcard) {
                                continue;
                            }
                            // The scrutinee is flattened; element `field_index` occupies a leaf
                            // sub-run at this offset — match its sub-pattern in place, no extraction.
                            let (start, _) =
                                self.codegen.tuple_elem_offset(&tuple_elems, field_index);
                            self.emit_sub_pattern(
                                sub_pat,
                                &tuple_elems[field_index],
                                scrutinee_local + start,
                                skip_depth,
                            );
                        }

                        if let Some(guard) = &arm.guard {
                            self.emit_expr(guard, ExprContext::Value);
                            self.instruction(Instruction::I32Eqz);
                            let br_depth = self.wasm_block_depth - skip_depth - 1;
                            self.instruction(Instruction::BrIf(br_depth));
                        }

                        self.emit_match_body(&arm.body, ctx);
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));

                        self.emit_end_block();
                        self.pop_scope();
                        // Conditional — never break, more arms may be needed
                    } else {
                        // All bindings/wildcards — always matches
                        for (field_index, sub_pat) in element_patterns.iter().enumerate() {
                            if matches!(sub_pat, TypedPattern::Wildcard) {
                                continue;
                            }
                            let (start, _) =
                                self.codegen.tuple_elem_offset(&tuple_elems, field_index);
                            self.emit_sub_pattern(
                                sub_pat,
                                &tuple_elems[field_index],
                                scrutinee_local + start,
                                0,
                            );
                        }

                        if let Some(guard) = &arm.guard {
                            self.emit_expr(guard, ExprContext::Value);
                            self.emit_if_block(BlockType::Empty);
                            self.emit_match_body(&arm.body, ctx);
                            let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                            self.instruction(Instruction::Br(br_depth));
                            self.emit_end_block();
                            self.pop_scope();
                        } else {
                            self.emit_match_body(&arm.body, ctx);
                            let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                            self.instruction(Instruction::Br(br_depth));
                            self.pop_scope();
                            break; // Unconditional match — skip remaining arms
                        }
                    }
                }
                TypedPattern::Newtype {
                    inner_pattern,
                    newtype_ty,
                } => {
                    let is_simple = matches!(
                        inner_pattern.as_ref(),
                        TypedPattern::Wildcard | TypedPattern::Variable(_, _)
                    );

                    self.push_scope();

                    if is_simple && arm.guard.is_none() {
                        // Unconditional: bind + emit body + break
                        if !matches!(inner_pattern.as_ref(), TypedPattern::Wildcard) {
                            self.emit_sub_pattern(
                                inner_pattern,
                                Self::newtype_inner_type(newtype_ty),
                                scrutinee_local,
                                0,
                            );
                        }
                        self.emit_match_body(&arm.body, ctx);
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.pop_scope();
                        break;
                    } else if is_simple {
                        // Simple inner + guard: bind, test guard, emit body
                        if !matches!(inner_pattern.as_ref(), TypedPattern::Wildcard) {
                            self.emit_sub_pattern(
                                inner_pattern,
                                Self::newtype_inner_type(newtype_ty),
                                scrutinee_local,
                                0,
                            );
                        }
                        self.emit_expr(arm.guard.as_ref().unwrap(), ExprContext::Value);
                        self.emit_if_block(BlockType::Empty);
                        self.emit_match_body(&arm.body, ctx);
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.emit_end_block();
                        self.pop_scope();
                    } else {
                        // Conditional inner (e.g., literal): skip block
                        self.emit_block(BlockType::Empty);
                        let skip_depth = self.wasm_block_depth - 1;

                        self.emit_sub_pattern(
                            inner_pattern,
                            Self::newtype_inner_type(newtype_ty),
                            scrutinee_local,
                            skip_depth,
                        );

                        if let Some(guard) = &arm.guard {
                            self.emit_expr(guard, ExprContext::Value);
                            self.instruction(Instruction::I32Eqz);
                            let br_depth = self.wasm_block_depth - skip_depth - 1;
                            self.instruction(Instruction::BrIf(br_depth));
                        }

                        self.emit_match_body(&arm.body, ctx);
                        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                        self.instruction(Instruction::Br(br_depth));
                        self.emit_end_block();
                        self.pop_scope();
                    }
                }
            }
        }

        // 4. After all arms: unreachable (exhaustiveness guarantees we never reach here)
        self.instruction(Instruction::Unreachable);

        // 5. End outer block
        self.emit_end_block();

        // A tuple result was spilled into result-temp locals by each arm; reload it.
        if let Some((base, valtypes)) = self.match_result_spill.take() {
            self.load_result_temp(base, &valtypes);
        }
        self.match_result_spill = prev_match_spill;
    }

    fn extract_interface_pattern_data(&mut self, subject_ty: &Type) {
        if let Type::InterfaceObject { mangled_name, .. } = subject_ty {
            self.instruction(Instruction::StructGet {
                struct_type_index: self.codegen.interface_object_type_indices[mangled_name],
                field_index: 0,
            });
        }
    }

    /// Emit a match arm body. When the match yields a tuple, the body's flattened values are
    /// spilled into the match's result-temp locals (the outer block is empty); otherwise the value
    /// flows out as the block result. An arm body has the same type as the whole match.
    fn emit_match_body(&mut self, body: &TypedExpr, ctx: ExprContext) {
        self.emit_expr(body, ctx);
        // A `Never`-typed arm body (e.g. an arm that is a `try`/early-return whose success
        // type is uninhabited) produces no real value — its WASM repr is a phantom `i32`.
        // The match block's result type, however, is the match's own type (often a
        // reference). Emitting `unreachable` makes the stack polymorphic so the following
        // `br` out of the match block validates regardless of the declared result type.
        if matches!(body.ty, Type::Never) {
            self.instruction(Instruction::Unreachable);
            return;
        }
        if let Some((base, valtypes)) = self.match_result_spill.clone() {
            self.store_value(base, &valtypes);
        }
    }

    /// Emit a record pattern match arm. Returns true if the arm is unconditional
    /// (all-binding with no guard), meaning the caller should break the arm loop.
    #[allow(clippy::too_many_arguments)]
    fn emit_record_pattern_arm(
        &mut self,
        fields: &[TypedFieldPattern],
        guard: &Option<Box<TypedExpr>>,
        body: &TypedExpr,
        subject_ty: &Type,
        scrutinee_local: u32,
        outer_block_depth: u32,
        ctx: ExprContext,
    ) -> bool {
        let struct_type_index = self.record_struct_info(subject_ty);
        let slot_types: Vec<Type> = self.record_slot_types(subject_ty);

        // Check if any field has a conditional sub-pattern (anything other than Variable/Wildcard)
        let has_conditional = fields.iter().any(|f| {
            !matches!(
                f.pattern,
                TypedPattern::Variable(_, _) | TypedPattern::Wildcard
            )
        });

        self.push_scope();

        if has_conditional {
            // Wrap in a block for early exit on sub-pattern mismatch
            self.emit_block(BlockType::Empty);
            let skip_depth = self.wasm_block_depth - 1;

            // Process all field sub-patterns
            for field in fields {
                if matches!(field.pattern, TypedPattern::Wildcard) {
                    continue;
                }
                let (field_val, field_ty) = self.extract_record_field(
                    subject_ty,
                    scrutinee_local,
                    struct_type_index,
                    field.field_index,
                    &slot_types[field.field_index as usize],
                );
                self.emit_sub_pattern(&field.pattern, &field_ty, field_val, skip_depth);
            }

            if let Some(guard_expr) = guard {
                self.emit_expr(guard_expr, ExprContext::Value);
                self.instruction(Instruction::I32Eqz);
                let br_depth = self.wasm_block_depth - skip_depth - 1;
                self.instruction(Instruction::BrIf(br_depth));
            }

            self.emit_match_body(body, ctx);
            let br_depth = self.wasm_block_depth - outer_block_depth - 1;
            self.instruction(Instruction::Br(br_depth));

            self.emit_end_block(); // end skip block
            self.pop_scope();
            false // conditional patterns
        } else {
            // All bindings/wildcards — always matches (like wildcard)
            for field in fields {
                if matches!(field.pattern, TypedPattern::Wildcard) {
                    continue;
                }
                let (field_val, field_ty) = self.extract_record_field(
                    subject_ty,
                    scrutinee_local,
                    struct_type_index,
                    field.field_index,
                    &slot_types[field.field_index as usize],
                );
                self.emit_sub_pattern(&field.pattern, &field_ty, field_val, 0); // skip_depth unused for Variable
            }

            if let Some(guard_expr) = guard {
                self.emit_expr(guard_expr, ExprContext::Value);
                self.emit_if_block(BlockType::Empty);
                self.emit_match_body(body, ctx);
                let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                self.instruction(Instruction::Br(br_depth));
                self.emit_end_block(); // end if
                self.pop_scope();
                false // guarded, so not unconditional
            } else {
                self.emit_match_body(body, ctx);
                let br_depth = self.wasm_block_depth - outer_block_depth - 1;
                self.instruction(Instruction::Br(br_depth));
                self.pop_scope();
                true // unconditional match — caller should break
            }
        }
    }

    /// Emit an enum variant pattern match arm.
    /// Always conditional (uses ref.test to check variant type).
    #[allow(clippy::too_many_arguments)]
    fn emit_enum_variant_pattern_arm(
        &mut self,
        enum_type: &Type,
        variant_name: &str,
        payload_patterns: &[TypedPattern],
        guard: &Option<Box<TypedExpr>>,
        body: &TypedExpr,
        scrutinee_local: u32,
        outer_block_depth: u32,
        ctx: ExprContext,
    ) {
        // Get the variant's WASM struct type index
        let enum_mn = match enum_type {
            Type::Enum(_, mn)
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => mn,
            _ => unreachable!("EnumVariant pattern must have Enum type"),
        };
        let variant_type_idx =
            self.codegen.variant_type_indices[&(enum_mn.clone(), variant_name.to_string())];

        self.push_scope();

        // Wrap in a skip block for early exit if not this variant
        self.emit_block(BlockType::Empty);
        let skip_depth = self.wasm_block_depth - 1;

        // ref.test: check if scrutinee is this variant subtype
        self.instruction(Instruction::LocalGet(scrutinee_local));
        self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
            variant_type_idx,
        )));
        self.instruction(Instruction::I32Eqz);
        let br_depth = self.wasm_block_depth - skip_depth - 1;
        self.instruction(Instruction::BrIf(br_depth));

        // ref.cast the scrutinee to the variant type and store in temp local
        let cast_valtype = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: HeapType::Concrete(variant_type_idx),
        });
        let cast_local = self.add_local(cast_valtype);
        self.instruction(Instruction::LocalGet(scrutinee_local));
        self.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            variant_type_idx,
        )));
        self.instruction(Instruction::LocalSet(cast_local));

        // Bind payload sub-patterns
        let payload_slot_types = self.enum_variant_payload_types(enum_type, variant_name);
        for (field_index, sub_pat) in payload_patterns.iter().enumerate() {
            if matches!(sub_pat, TypedPattern::Wildcard) {
                continue;
            }
            let (field_val, field_ty) = self.extract_enum_payload(
                enum_type,
                variant_name,
                cast_local,
                variant_type_idx,
                field_index,
                &payload_slot_types[field_index],
            );
            self.emit_sub_pattern(sub_pat, &field_ty, field_val, skip_depth);
        }

        // Handle guard
        if let Some(guard_expr) = guard {
            self.emit_expr(guard_expr, ExprContext::Value);
            self.instruction(Instruction::I32Eqz);
            let br_depth = self.wasm_block_depth - skip_depth - 1;
            self.instruction(Instruction::BrIf(br_depth));
        }

        // Emit body and break to outer block
        self.emit_match_body(body, ctx);
        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
        self.instruction(Instruction::Br(br_depth));

        self.emit_end_block(); // end skip block
        self.pop_scope();
    }

    /// Emit an enum variant record-style pattern match arm.
    /// Uses field_index from TypedFieldPattern for correct struct.get offsets.
    #[allow(clippy::too_many_arguments)]
    fn emit_enum_variant_record_pattern_arm(
        &mut self,
        enum_type: &Type,
        variant_name: &str,
        field_patterns: &[TypedFieldPattern],
        guard: &Option<Box<TypedExpr>>,
        body: &TypedExpr,
        scrutinee_local: u32,
        outer_block_depth: u32,
        ctx: ExprContext,
    ) {
        let enum_mn = match enum_type {
            Type::Enum(_, mn)
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => mn,
            _ => unreachable!("EnumVariant pattern must have Enum type"),
        };
        let variant_type_idx =
            self.codegen.variant_type_indices[&(enum_mn.clone(), variant_name.to_string())];

        self.push_scope();

        self.emit_block(BlockType::Empty);
        let skip_depth = self.wasm_block_depth - 1;

        // ref.test: check if scrutinee is this variant subtype
        self.instruction(Instruction::LocalGet(scrutinee_local));
        self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
            variant_type_idx,
        )));
        self.instruction(Instruction::I32Eqz);
        let br_depth = self.wasm_block_depth - skip_depth - 1;
        self.instruction(Instruction::BrIf(br_depth));

        // ref.cast the scrutinee to the variant type
        let cast_valtype = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: HeapType::Concrete(variant_type_idx),
        });
        let cast_local = self.add_local(cast_valtype);
        self.instruction(Instruction::LocalGet(scrutinee_local));
        self.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            variant_type_idx,
        )));
        self.instruction(Instruction::LocalSet(cast_local));

        // Bind field patterns using their field_index for struct.get
        let payload_slot_types = self.enum_variant_payload_types(enum_type, variant_name);
        for fp in field_patterns {
            if matches!(fp.pattern, TypedPattern::Wildcard) {
                continue;
            }
            let (field_val, field_ty) = self.extract_enum_payload(
                enum_type,
                variant_name,
                cast_local,
                variant_type_idx,
                fp.field_index as usize,
                &payload_slot_types[fp.field_index as usize],
            );
            self.emit_sub_pattern(&fp.pattern, &field_ty, field_val, skip_depth);
        }

        // Handle guard
        if let Some(guard_expr) = guard {
            self.emit_expr(guard_expr, ExprContext::Value);
            self.instruction(Instruction::I32Eqz);
            let br_depth = self.wasm_block_depth - skip_depth - 1;
            self.instruction(Instruction::BrIf(br_depth));
        }

        // Emit body and break to outer block
        self.emit_match_body(body, ctx);
        let br_depth = self.wasm_block_depth - outer_block_depth - 1;
        self.instruction(Instruction::Br(br_depth));

        self.emit_end_block();
        self.pop_scope();
    }

    /// Emit a sub-pattern match for a value that lives *flattened* in locals
    /// `value_base .. value_base + flat_width(pattern type)`: a tuple is a run of leaf locals (no
    /// boxing), a scalar/ref is a single local. On mismatch, branches to `skip_depth`.
    fn emit_sub_pattern(
        &mut self,
        sub_pat: &TypedPattern,
        source_ty: &Type,
        value_base: u32,
        skip_depth: u32,
    ) {
        match sub_pat {
            TypedPattern::Wildcard => {} // always matches, nothing to do
            TypedPattern::Variable(name, _) => {
                // The value already lives flattened at `value_base`; alias the binding to its run
                // (read back via `type_to_valtypes(ty)`). No copy, no box/unbox.
                self.bind_name(name.clone(), value_base);
            }
            TypedPattern::TypeAnnotated { binding, ty } => {
                self.emit_type_pattern(binding, ty, source_ty, value_base, skip_depth);
            }
            TypedPattern::Literal(lit_expr) => {
                self.instruction(Instruction::LocalGet(value_base));
                self.emit_expr(lit_expr, ExprContext::Value);
                self.emit_eq_instruction(&lit_expr.ty);
                self.instruction(Instruction::I32Eqz);
                let br_depth = self.wasm_block_depth - skip_depth - 1;
                self.instruction(Instruction::BrIf(br_depth));
            }
            TypedPattern::Newtype {
                inner_pattern,
                newtype_ty,
            } => {
                // Transparent — the newtype's flattened representation is the inner value's.
                if !matches!(inner_pattern.as_ref(), TypedPattern::Wildcard) {
                    self.emit_sub_pattern(
                        inner_pattern,
                        Self::newtype_inner_type(newtype_ty),
                        value_base,
                        skip_depth,
                    );
                }
            }
            TypedPattern::Tuple {
                element_patterns,
                tuple_type,
            } => {
                let tuple_elems = match tuple_type {
                    Type::Tuple(elems, _) => elems.clone(),
                    _ => unreachable!("Tuple pattern must have Tuple type"),
                };
                // The tuple value is a flattened run at `value_base`; each element occupies a leaf
                // sub-run — match its sub-pattern in place.
                for (field_index, sub_sub_pat) in element_patterns.iter().enumerate() {
                    if matches!(sub_sub_pat, TypedPattern::Wildcard) {
                        continue;
                    }
                    let (start, _) = self.codegen.tuple_elem_offset(&tuple_elems, field_index);
                    self.emit_sub_pattern(
                        sub_sub_pat,
                        &tuple_elems[field_index],
                        value_base + start,
                        skip_depth,
                    );
                }
            }
            TypedPattern::EnumVariant {
                enum_type,
                variant_name,
                payload_patterns,
                ..
            } => {
                let variant_type_idx = self.variant_index(enum_type, variant_name);
                // value_base holds the single enum ref. ref.test → skip on mismatch.
                self.instruction(Instruction::LocalGet(value_base));
                self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
                    variant_type_idx,
                )));
                self.instruction(Instruction::I32Eqz);
                let br_depth = self.wasm_block_depth - skip_depth - 1;
                self.instruction(Instruction::BrIf(br_depth));

                if !payload_patterns.is_empty() {
                    let cast_local = self.cast_to_variant(value_base, variant_type_idx);
                    let payload_slot_types =
                        self.enum_variant_payload_types(enum_type, variant_name);
                    for (field_index, sub_sub_pat) in payload_patterns.iter().enumerate() {
                        if matches!(sub_sub_pat, TypedPattern::Wildcard) {
                            continue;
                        }
                        let (field_base, field_ty) = self.extract_enum_payload(
                            enum_type,
                            variant_name,
                            cast_local,
                            variant_type_idx,
                            field_index,
                            &payload_slot_types[field_index],
                        );
                        self.emit_sub_pattern(sub_sub_pat, &field_ty, field_base, skip_depth);
                    }
                }
            }
            TypedPattern::EnumVariantRecord {
                enum_type,
                variant_name,
                field_patterns,
                ..
            } => {
                let variant_type_idx = self.variant_index(enum_type, variant_name);
                self.instruction(Instruction::LocalGet(value_base));
                self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
                    variant_type_idx,
                )));
                self.instruction(Instruction::I32Eqz);
                let br_depth = self.wasm_block_depth - skip_depth - 1;
                self.instruction(Instruction::BrIf(br_depth));

                if !field_patterns.is_empty() {
                    let cast_local = self.cast_to_variant(value_base, variant_type_idx);
                    let payload_slot_types =
                        self.enum_variant_payload_types(enum_type, variant_name);
                    for fp in field_patterns {
                        if matches!(fp.pattern, TypedPattern::Wildcard) {
                            continue;
                        }
                        let (field_base, field_ty) = self.extract_enum_payload(
                            enum_type,
                            variant_name,
                            cast_local,
                            variant_type_idx,
                            fp.field_index as usize,
                            &payload_slot_types[fp.field_index as usize],
                        );
                        self.emit_sub_pattern(&fp.pattern, &field_ty, field_base, skip_depth);
                    }
                }
            }
            TypedPattern::Record { ty, fields } => {
                let struct_type_index = self.record_struct_info(ty);
                let slot_types = self.record_slot_types(ty);
                for fp in fields {
                    if matches!(fp.pattern, TypedPattern::Wildcard) {
                        continue;
                    }
                    let (field_base, field_ty) = self.extract_record_field(
                        ty,
                        value_base,
                        struct_type_index,
                        fp.field_index,
                        &slot_types[fp.field_index as usize],
                    );
                    self.emit_sub_pattern(&fp.pattern, &field_ty, field_base, skip_depth);
                }
            }
        }
    }

    /// WASM struct type index of enum variant `variant_name`.
    fn variant_index(&self, enum_type: &Type, variant_name: &str) -> u32 {
        let enum_mn = match enum_type {
            Type::Enum(_, mn)
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => mn,
            _ => unreachable!("EnumVariant pattern must have Enum type"),
        };
        self.codegen.variant_type_indices[&(enum_mn.clone(), variant_name.to_string())]
    }

    /// `ref.cast` the enum ref in `value_base` to `variant_type_idx` and store it in a fresh local,
    /// returning that local (the typed variant struct, for payload `struct.get`s).
    fn cast_to_variant(&mut self, value_base: u32, variant_type_idx: u32) -> u32 {
        let cast_valtype = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: HeapType::Concrete(variant_type_idx),
        });
        let cast_local = self.add_local(cast_valtype);
        self.instruction(Instruction::LocalGet(value_base));
        self.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            variant_type_idx,
        )));
        self.instruction(Instruction::LocalSet(cast_local));
        cast_local
    }

    /// The MangledName of a record type (for `struct_field_range`).
    fn record_mn(ty: &Type) -> crate::common::types::MangledName {
        match ty {
            Type::Record(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            } => mn.clone(),
            _ => unreachable!("record pattern on non-record type: {ty}"),
        }
    }

    /// Extract a Dovetail field/payload occupying WASM fields `[start, start+width)` of `struct_local`
    /// into a fresh run of locals holding the field's *flattened* value (`type_to_valtypes(target_ty)`),
    /// returning its base. A concrete spliced field reads its `width` leaf fields directly. An erased
    /// (anyref) slot reads the single boxed slot and casts it back to the concrete target — a tuple
    /// explodes into its flattened leaves, a primitive unboxes, a reference casts; an `Any`/erased
    /// target keeps the raw anyref.
    fn extract_field_run(
        &mut self,
        struct_local: u32,
        struct_type_index: u32,
        start: u32,
        width: u32,
        slot_ty: &Type,
        target_ty: &Type,
    ) -> u32 {
        let target_valtypes = self.codegen.type_to_valtypes(target_ty);
        let base = self.add_value_locals(&target_valtypes);
        if crate::codegen::Codegen::is_erased_slot(slot_ty) {
            self.instruction(Instruction::LocalGet(struct_local));
            self.instruction(Instruction::StructGet {
                struct_type_index,
                field_index: start,
            });
            if matches!(target_ty, Type::Never | Type::Error) {
                // Binding to an uninhabited / error type — `unreachable` is polymorphic, so the
                // following `local.set`s validate.
                self.instruction(Instruction::Unreachable);
            } else if !matches!(target_ty, Type::Any)
                && !crate::codegen::Codegen::is_erased_slot(target_ty)
            {
                // Concrete target: cast back from the anyref slot. A tuple explodes into its leaves.
                self.emit_cast_back_from_any(target_ty);
            }
            // else: `Any` / still-erased target — keep the raw anyref.
        } else {
            // Read the declared flattened layout, then restore concrete leaves.
            // coerce_value also follows transparent newtypes such as Slice<T>.
            for k in 0..width {
                self.instruction(Instruction::LocalGet(struct_local));
                self.instruction(Instruction::StructGet {
                    struct_type_index,
                    field_index: start + k,
                });
            }
            self.coerce_value(slot_ty, target_ty);
        }
        self.store_value(base, &target_valtypes);
        base
    }

    /// Extract record field `field_index` (splice-aware) into a run, returning its base.
    fn extract_record_field(
        &mut self,
        record_ty: &Type,
        struct_local: u32,
        struct_type_index: u32,
        field_index: u32,
        slot_ty: &Type,
    ) -> (u32, Type) {
        let record_mn = Self::record_mn(record_ty);
        let (start, width) = self
            .codegen
            .struct_field_range(&record_mn, field_index as usize);
        let field_ty = self.pattern_slot_type(record_ty, slot_ty);
        let base = self.extract_field_run(
            struct_local,
            struct_type_index,
            start,
            width,
            slot_ty,
            &field_ty,
        );
        (base, field_ty)
    }

    /// Extract payload `field_index` of an enum variant struct held in `struct_local`, handling
    /// spliced tuple payloads via the field range.
    fn extract_enum_payload(
        &mut self,
        enum_type: &Type,
        variant_name: &str,
        struct_local: u32,
        variant_type_index: u32,
        field_index: usize,
        slot_ty: &Type,
    ) -> (u32, Type) {
        let enum_mn = match enum_type {
            Type::Enum(_, mn)
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => mn.clone(),
            _ => unreachable!("extract_enum_payload: expected Enum, got {enum_type}"),
        };
        let (start, width) = self
            .codegen
            .enum_payload_range(&enum_mn, variant_name, field_index);
        let field_ty = self.pattern_slot_type(enum_type, slot_ty);
        let base = self.extract_field_run(
            struct_local,
            variant_type_index,
            start,
            width,
            slot_ty,
            &field_ty,
        );
        (base, field_ty)
    }

    /// Restore the subject field type before applying a potentially narrower pattern.
    fn pattern_slot_type(&self, container: &Type, slot_ty: &Type) -> Type {
        let type_args = match container {
            Type::GenericRecord { type_args, .. } | Type::GenericEnum { type_args, .. } => {
                type_args
            }
            _ => return slot_ty.clone(),
        };
        let params = match &self.codegen.typed_module.types[&container.mangled_name()] {
            crate::typechecker::types::TypeDef::Record(r) => &r.type_params,
            crate::typechecker::types::TypeDef::Enum(e) => &e.type_params,
            _ => unreachable!("pattern slot requires a record or enum"),
        };
        let bindings = params
            .iter()
            .cloned()
            .zip(type_args.iter().map(|(_, ty)| ty.clone()))
            .collect();
        crate::monomorphize::substitute::apply_type_substitution(slot_ty, &bindings)
    }

    fn newtype_inner_type(ty: &Type) -> &Type {
        match ty {
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => inner,
            _ => unreachable!("newtype pattern requires a newtype subject"),
        }
    }

    /// Both top-level and nested annotations test the source representation before binding.
    fn emit_type_pattern(
        &mut self,
        binding: &crate::common::types::VarName,
        target: &Type,
        source: &Type,
        base: u32,
        skip_depth: u32,
    ) {
        let dynamic = source.is_any()
            || source.is_class_type()
            || matches!(
                source,
                Type::InterfaceObject { .. }
                    | Type::GenericRecord { .. }
                    | Type::GenericEnum { .. }
            );
        if !dynamic {
            self.bind_name(binding.clone(), base);
            return;
        }
        self.instruction(Instruction::LocalGet(base));
        self.extract_interface_pattern_data(source);
        self.emit_runtime_type_test(target);
        self.instruction(Instruction::I32Eqz);
        let depth = self.wasm_block_depth - skip_depth - 1;
        self.instruction(Instruction::BrIf(depth));
        self.instruction(Instruction::LocalGet(base));
        self.extract_interface_pattern_data(source);
        self.emit_cast_back_from_any(target);
        let valtypes = self.codegen.type_to_valtypes(target);
        let local = self.define_value_locals(binding.clone(), &valtypes);
        self.store_value(local, &valtypes);
    }

    /// Get the WASM struct type index for a record type.
    fn record_struct_info(&self, ty: &Type) -> u32 {
        match ty {
            Type::Record(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            } => self.codegen.type_indices[mn],
            _ => unreachable!("Record pattern must have Record scrutinee type"),
        }
    }

    /// Look up the declared field types of a Record/Tuple type's TypeDef.
    fn record_slot_types(&self, ty: &Type) -> Vec<Type> {
        match ty {
            Type::Record(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            } => match &self.codegen.typed_module.types[mn] {
                crate::typechecker::types::TypeDef::Record(r) => {
                    r.fields.iter().map(|(_, t)| t.clone()).collect()
                }
                _ => unreachable!("record_slot_types: not a Record TypeDef"),
            },
            Type::Tuple(elems, _) => elems.clone(),
            _ => unreachable!("record_slot_types: expected Record/Tuple, got {}", ty),
        }
    }

    /// Look up an enum variant's payload types.
    pub(super) fn enum_variant_payload_types(
        &self,
        enum_ty: &Type,
        variant_name: &str,
    ) -> Vec<Type> {
        let mn = match enum_ty {
            Type::Enum(_, mn)
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => mn,
            _ => unreachable!("enum_variant_payload_types: expected Enum, got {}", enum_ty),
        };
        match &self.codegen.typed_module.types[mn] {
            crate::typechecker::types::TypeDef::Enum(e) => e
                .variants
                .iter()
                .find(|v| v.name == variant_name)
                .map(|v| v.payload_types.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}
