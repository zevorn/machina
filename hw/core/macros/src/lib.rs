use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, spanned::Spanned, Attribute, Data, DeriveInput, Error,
    Expr, ExprPath, Field, Fields, Ident, LitStr, Result, Type,
};

#[proc_macro_derive(SysBusDevice, attributes(mom))]
pub fn derive_sysbus_device(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_sysbus_device(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(MDevice, attributes(mom))]
pub fn derive_mdevice(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_mdevice(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(MProperties, attributes(property))]
pub fn derive_mproperties(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_mproperties(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Resettable, attributes(reset))]
pub fn derive_resettable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_resettable(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand_sysbus_device(input: &DeriveInput) -> Result<TokenStream2> {
    let name = &input.ident;
    let args = MomArgs::parse(&input.attrs)?;
    let state = args.state_ident()?;
    let lock_kind = args.lock_kind()?;
    if args.lock_fn.is_some() && lock_kind != LockKind::Std {
        return Err(Error::new(
            state.span(),
            "lock_fn is only supported with lock = \"std\"",
        ));
    }
    let macro_name = match lock_kind {
        LockKind::ParkingLot => quote! {
            ::machina_hw_core::machina_parking_lot_sysbus_accessors
        },
        LockKind::Std => quote! {
            ::machina_hw_core::machina_std_mutex_sysbus_accessors
        },
    };
    let accessor = sysbus_accessor_invocation(&macro_name, state, &args)?;

    Ok(quote! {
        impl #name {
            #accessor
        }
    })
}

fn expand_mdevice(input: &DeriveInput) -> Result<TokenStream2> {
    let name = &input.ident;
    let args = MomArgs::parse(&input.attrs)?;
    let state = args.state_ident()?;
    let accessor = match args.lock_kind()? {
        LockKind::ParkingLot => quote! {
            ::machina_hw_core::machina_parking_lot_mdevice_accessors!(#state);
        },
        LockKind::Std => quote! {
            ::machina_hw_core::machina_std_mutex_mdevice_accessors!(#state);
        },
    };

    if args.has_sysbus_only_options() {
        return Err(Error::new(
            input.span(),
            "MDevice derive supports only #[mom(state = ..., lock = ...)]",
        ));
    }

    Ok(quote! {
        impl #name {
            #accessor
        }
    })
}

fn expand_mproperties(input: &DeriveInput) -> Result<TokenStream2> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.span(),
            "MProperties can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new(
            data.fields.span(),
            "MProperties requires named fields",
        ));
    };

    let specs = fields
        .named
        .iter()
        .filter_map(property_spec_for_field)
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl #name {
            pub fn property_specs() -> ::std::vec::Vec<
                ::machina_hw_core::property::MPropertySpec
            > {
                ::std::vec![#(#specs),*]
            }
        }
    })
}

fn expand_resettable(input: &DeriveInput) -> Result<TokenStream2> {
    let name = &input.ident;
    let args = ResetArgs::parse(&input.attrs)?;
    let enter = reset_method("reset_enter", &args.enter);
    let hold = reset_method("reset_hold", &args.hold);
    let exit = reset_method("reset_exit", &args.exit);

    Ok(quote! {
        impl ::machina_hw_core::reset::Resettable for #name {
            #enter
            #hold
            #exit
        }
    })
}

fn reset_method(name: &str, target: &Option<Ident>) -> TokenStream2 {
    let Some(target) = target else {
        return TokenStream2::new();
    };
    let method = Ident::new(name, target.span());
    quote! {
        fn #method(&self, _phase: ::machina_hw_core::reset::ResetPhase) {
            self.#target();
        }
    }
}

fn sysbus_accessor_invocation(
    macro_name: &TokenStream2,
    state: &Ident,
    args: &MomArgs,
) -> Result<TokenStream2> {
    if let Some(lock_fn) = &args.lock_fn {
        if args.has_sysbus_hooks_or_modes() {
            return Err(Error::new(
                lock_fn.span(),
                "lock_fn can only be combined with the default sysbus accessor",
            ));
        }
        return Ok(quote! {
            #macro_name!(#state, lock = #lock_fn);
        });
    }

    if args.lifecycle_manual {
        if args.has_sysbus_hooks_or_irq_mode() {
            return Err(Error::new(
                state.span(),
                "lifecycle = \"manual\" cannot be combined with sysbus hooks",
            ));
        }
        return Ok(quote! {
            #macro_name!(#state, lifecycle = manual);
        });
    }

    if let (Some(before_register_mmio), Some(before_realize)) =
        (&args.before_register_mmio, &args.before_realize)
    {
        if args.irq_manual {
            return Err(Error::new(
                before_register_mmio.span(),
                "irq = \"manual\" cannot be combined with before_register_mmio",
            ));
        }
        if args.before_unrealize.is_empty() {
            return Ok(quote! {
                #macro_name!(
                    #state,
                    before_register_mmio = #before_register_mmio,
                    before_realize = #before_realize
                );
            });
        }
        let before_unrealize = before_unrealize_tokens(&args.before_unrealize);
        return Ok(quote! {
            #macro_name!(
                #state,
                before_register_mmio = #before_register_mmio,
                before_realize = #before_realize,
                before_unrealize = #before_unrealize
            );
        });
    }

    if args.before_register_mmio.is_some() || args.before_realize.is_some() {
        return Err(Error::new(
            state.span(),
            "before_register_mmio and before_realize must be specified together",
        ));
    }

    if args.irq_manual {
        if args.before_unrealize.is_empty() {
            return Ok(quote! {
                #macro_name!(#state, irq = manual);
            });
        }
        let before_unrealize = before_unrealize_tokens(&args.before_unrealize);
        return Ok(quote! {
            #macro_name!(
                #state,
                irq = manual,
                before_unrealize = #before_unrealize
            );
        });
    }

    if !args.before_unrealize.is_empty() {
        let before_unrealize = before_unrealize_tokens(&args.before_unrealize);
        return Ok(quote! {
            #macro_name!(#state, before_unrealize = #before_unrealize);
        });
    }

    Ok(quote! {
        #macro_name!(#state);
    })
}

fn before_unrealize_tokens(methods: &[Ident]) -> TokenStream2 {
    if let [method] = methods {
        quote! { #method }
    } else {
        quote! { [#(#methods),*] }
    }
}

fn property_spec_for_field(field: &Field) -> Option<Result<TokenStream2>> {
    let attr = field
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("property"))?;
    Some(property_spec_for_attr(field, attr))
}

fn property_spec_for_attr(
    field: &Field,
    attr: &Attribute,
) -> Result<TokenStream2> {
    let field_name = field.ident.as_ref().ok_or_else(|| {
        Error::new(field.span(), "property fields must be named")
    })?;
    let mut args = PropertyArgs::default();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("rename") || meta.path.is_ident("name") {
            args.rename = Some(meta.value()?.parse()?);
            return Ok(());
        }
        if meta.path.is_ident("kind") {
            args.kind = Some(parse_string_or_ident(meta.value()?)?);
            return Ok(());
        }
        if meta.path.is_ident("default") {
            args.default = Some(meta.value()?.parse()?);
            return Ok(());
        }
        if meta.path.is_ident("required") {
            args.required = true;
            return Ok(());
        }
        if meta.path.is_ident("dynamic") {
            args.dynamic = true;
            return Ok(());
        }
        Err(meta.error("unsupported property option"))
    })?;

    let property_name = args
        .rename
        .map_or_else(|| field_name.to_string(), |rename| rename.value());
    let property_type = property_type_tokens(args.kind.as_deref(), &field.ty)?;
    let mut spec = quote! {
        ::machina_hw_core::property::MPropertySpec::new(#property_name, #property_type)
    };

    if args.required {
        spec = quote! { #spec.required() };
    }
    if args.dynamic {
        spec = quote! { #spec.dynamic() };
    }
    if let Some(default) = args.default {
        let default =
            property_default_tokens(args.kind.as_deref(), &field.ty, &default)?;
        spec = quote! { #spec.default(#default) };
    }

    Ok(spec)
}

fn property_type_tokens(
    kind: Option<&str>,
    field_ty: &Type,
) -> Result<TokenStream2> {
    let inferred = kind
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| infer_property_kind(field_ty));
    match inferred.as_deref() {
        Some("bool") => {
            Ok(quote! { ::machina_hw_core::property::MPropertyType::Bool })
        }
        Some("u32") => {
            Ok(quote! { ::machina_hw_core::property::MPropertyType::U32 })
        }
        Some("u64") => {
            Ok(quote! { ::machina_hw_core::property::MPropertyType::U64 })
        }
        Some("string") => {
            Ok(quote! { ::machina_hw_core::property::MPropertyType::String })
        }
        Some("link") => {
            Ok(quote! { ::machina_hw_core::property::MPropertyType::Link })
        }
        Some(other) => Err(Error::new(
            field_ty.span(),
            format!("unsupported property kind '{other}'"),
        )),
        None => Err(Error::new(
            field_ty.span(),
            "cannot infer property kind; use #[property(kind = \"...\")]",
        )),
    }
}

fn property_default_tokens(
    kind: Option<&str>,
    field_ty: &Type,
    default: &Expr,
) -> Result<TokenStream2> {
    let inferred = kind
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| infer_property_kind(field_ty));
    match inferred.as_deref() {
        Some("bool") => Ok(quote! {
            ::machina_hw_core::property::MPropertyValue::Bool(#default)
        }),
        Some("u32") => Ok(quote! {
            ::machina_hw_core::property::MPropertyValue::U32(#default)
        }),
        Some("u64") => Ok(quote! {
            ::machina_hw_core::property::MPropertyValue::U64(#default)
        }),
        Some("string") => Ok(quote! {
            ::machina_hw_core::property::MPropertyValue::String(
                ::std::string::String::from(#default)
            )
        }),
        Some("link") => Ok(quote! {
            ::machina_hw_core::property::MPropertyValue::Link(
                ::std::string::String::from(#default)
            )
        }),
        _ => Err(Error::new(
            field_ty.span(),
            "cannot build property default for this field type",
        )),
    }
}

fn infer_property_kind(field_ty: &Type) -> Option<String> {
    let Type::Path(path) = field_ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    match segment.ident.to_string().as_str() {
        "bool" => Some("bool".to_string()),
        "u32" => Some("u32".to_string()),
        "u64" => Some("u64".to_string()),
        "String" => Some("string".to_string()),
        _ => None,
    }
}

#[derive(Default)]
struct MomArgs {
    state: Option<Ident>,
    lock: Option<LockKind>,
    lock_fn: Option<Ident>,
    lifecycle_manual: bool,
    irq_manual: bool,
    before_register_mmio: Option<Ident>,
    before_realize: Option<Ident>,
    before_unrealize: Vec<Ident>,
}

impl MomArgs {
    fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut args = Self::default();
        let attr = attrs
            .iter()
            .find(|attr| attr.path().is_ident("mom"))
            .ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "missing #[mom(...)]",
                )
            })?;

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("state") {
                args.state = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("lock") {
                let value = parse_string_or_ident(meta.value()?)?;
                args.lock = Some(LockKind::parse(&value, meta.path.span())?);
                return Ok(());
            }
            if meta.path.is_ident("lock_fn") {
                args.lock_fn = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("lifecycle") {
                let value = parse_string_or_ident(meta.value()?)?;
                args.lifecycle_manual = parse_manual(&value, meta.path.span())?;
                return Ok(());
            }
            if meta.path.is_ident("irq") {
                let value = parse_string_or_ident(meta.value()?)?;
                args.irq_manual = parse_manual(&value, meta.path.span())?;
                return Ok(());
            }
            if meta.path.is_ident("before_register_mmio") {
                args.before_register_mmio = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("before_realize") {
                args.before_realize = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("before_unrealize") {
                let expr: Expr = meta.value()?.parse()?;
                args.before_unrealize = parse_method_list(expr)?;
                return Ok(());
            }
            Err(meta.error("unsupported mom option"))
        })?;

        Ok(args)
    }

    fn state_ident(&self) -> Result<&Ident> {
        self.state.as_ref().ok_or_else(|| {
            Error::new(
                proc_macro2::Span::call_site(),
                "missing mom state field",
            )
        })
    }

    fn lock_kind(&self) -> Result<LockKind> {
        self.lock.ok_or_else(|| {
            Error::new(
                proc_macro2::Span::call_site(),
                "missing mom lock kind; use lock = \"std\" or lock = \"parking_lot\"",
            )
        })
    }

    fn has_sysbus_only_options(&self) -> bool {
        self.lock_fn.is_some()
            || self.lifecycle_manual
            || self.irq_manual
            || self.before_register_mmio.is_some()
            || self.before_realize.is_some()
            || !self.before_unrealize.is_empty()
    }

    fn has_sysbus_hooks_or_modes(&self) -> bool {
        self.lifecycle_manual || self.has_sysbus_hooks_or_irq_mode()
    }

    fn has_sysbus_hooks_or_irq_mode(&self) -> bool {
        self.irq_manual
            || self.before_register_mmio.is_some()
            || self.before_realize.is_some()
            || !self.before_unrealize.is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LockKind {
    ParkingLot,
    Std,
}

impl LockKind {
    fn parse(value: &str, span: proc_macro2::Span) -> Result<Self> {
        match value {
            "parking_lot" => Ok(Self::ParkingLot),
            "std" => Ok(Self::Std),
            other => Err(Error::new(
                span,
                format!("unsupported lock kind '{other}'"),
            )),
        }
    }
}

#[derive(Default)]
struct PropertyArgs {
    rename: Option<LitStr>,
    kind: Option<String>,
    default: Option<Expr>,
    required: bool,
    dynamic: bool,
}

#[derive(Default)]
struct ResetArgs {
    enter: Option<Ident>,
    hold: Option<Ident>,
    exit: Option<Ident>,
}

impl ResetArgs {
    fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut args = Self::default();
        let Some(attr) =
            attrs.iter().find(|attr| attr.path().is_ident("reset"))
        else {
            return Ok(args);
        };

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("enter") {
                args.enter = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("hold") {
                args.hold = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("exit") {
                args.exit = Some(meta.value()?.parse()?);
                return Ok(());
            }
            Err(meta.error("unsupported reset option"))
        })?;

        Ok(args)
    }
}

fn parse_manual(value: &str, span: proc_macro2::Span) -> Result<bool> {
    if value == "manual" {
        Ok(true)
    } else {
        Err(Error::new(
            span,
            format!("unsupported mode '{value}', expected 'manual'"),
        ))
    }
}

fn parse_method_list(expr: Expr) -> Result<Vec<Ident>> {
    match expr {
        Expr::Path(path) => Ok(vec![path_ident(&path)?]),
        Expr::Array(array) => array.elems.into_iter().map(expr_ident).collect(),
        other => Err(Error::new(
            other.span(),
            "expected method name or [method, ...]",
        )),
    }
}

fn expr_ident(expr: Expr) -> Result<Ident> {
    match expr {
        Expr::Path(path) => path_ident(&path),
        other => Err(Error::new(other.span(), "expected method name")),
    }
}

fn path_ident(path: &ExprPath) -> Result<Ident> {
    path.path.get_ident().cloned().ok_or_else(|| {
        Error::new(path.span(), "expected unqualified method name")
    })
}

fn parse_string_or_ident(input: syn::parse::ParseStream<'_>) -> Result<String> {
    if input.peek(LitStr) {
        return input.parse::<LitStr>().map(|lit| lit.value());
    }
    input.parse::<Ident>().map(|ident| ident.to_string())
}
