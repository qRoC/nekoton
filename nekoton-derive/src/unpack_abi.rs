use quote::quote;

use crate::ast::*;
use crate::attr::TypeName;
use crate::parsing_context::*;
use crate::utils::*;

pub fn impl_derive_unpack_abi(
    input: syn::DeriveInput,
    plain: bool,
) -> Result<proc_macro2::TokenStream, Vec<syn::Error>> {
    let cx = ParsingContext::new();
    let container = match Container::from_ast(&cx, &input) {
        Some(container) => container,
        None => return Err(cx.check().unwrap_err()),
    };

    if plain && matches!(&container.data, Data::Enum(_)) {
        cx.error_spanned_by(&input.ident, "Plain unpacker is not supported for enums");
    }

    cx.check()?;

    let ident = &container.ident;
    let result = match &container.data {
        Data::Enum(variants) => {
            let enum_type = if container.attrs.enum_bool {
                EnumType::Bool
            } else {
                EnumType::Int
            };
            let body = serialize_enum(&container, variants, enum_type);
            quote! {
                impl ::nekoton_abi::UnpackAbi<#ident> for ::ton_abi::TokenValue {
                    fn unpack(self) -> ::nekoton_abi::UnpackerResult<#ident> {
                        #body
                    }
                }
            }
        }
        Data::Struct(_, fields) => {
            if plain {
                let body = serialize_struct(&container, fields, StructType::Plain);
                quote! {
                    impl ::nekoton_abi::UnpackAbiPlain<#ident> for Vec<::ton_abi::Token> {
                        fn unpack(self) -> ::nekoton_abi::UnpackerResult<#ident> {
                            #body
                        }
                    }
                }
            } else {
                let body = serialize_struct(&container, fields, StructType::Tuple);
                quote! {
                    impl ::nekoton_abi::UnpackAbi<#ident> for ::ton_abi::TokenValue {
                        fn unpack(self) -> ::nekoton_abi::UnpackerResult<#ident> {
                            #body
                        }
                    }
                }
            }
        }
    };
    Ok(result)
}

fn serialize_enum(
    container: &Container<'_>,
    variants: &[Variant<'_>],
    enum_type: EnumType,
) -> proc_macro2::TokenStream {
    let name = &container.ident;

    let build_variants = variants
        .iter()
        .filter_map(|variant| {
            variant
                .original
                .discriminant
                .as_ref()
                .map(|(_, discriminant)| (variant.ident.clone(), discriminant))
        })
        .map(|(ident, discriminant)| {
            let token = quote::ToTokens::to_token_stream(discriminant).to_string();
            let number = token.parse::<u8>().unwrap();

            match enum_type {
                EnumType::Int => {
                    quote! {
                        Some(#number) => Ok(#name::#ident)
                    }
                }
                EnumType::Bool => {
                    if number == 0 {
                        quote! {
                            ::ton_abi::TokenValue::Bool(false) => Ok(#name::#ident)
                        }
                    } else {
                        quote! {
                            ::ton_abi::TokenValue::Bool(true) => Ok(#name::#ident)
                        }
                    }
                }
            }
        });

    match enum_type {
        EnumType::Int => {
            quote! {
                match self {
                    ::ton_abi::TokenValue::Uint(int) => match ::nekoton_abi::num_traits::ToPrimitive::to_u8(&int.number) {
                        #(#build_variants,)*
                        _ => Err(::nekoton_abi::UnpackerError::InvalidAbi),
                    },
                    _ => Err(::nekoton_abi::UnpackerError::InvalidAbi),
                }
            }
        }
        EnumType::Bool => {
            quote! {
                match self {
                    #(#build_variants,)*
                    _ => Err(::nekoton_abi::UnpackerError::InvalidAbi),
                }
            }
        }
    }
}

fn serialize_struct(
    container: &Container<'_>,
    fields: &[Field<'_>],
    struct_type: StructType,
) -> proc_macro2::TokenStream {
    let name = &container.ident;

    let build_fields = fields.iter().map(|f| {
        let name = f.original.ident.as_ref().unwrap();

        if f.attrs.skip {
            quote! {
               #name: std::default::Default::default()
            }
        } else {
            let try_unpack = try_unpack(
                &f.attrs.type_name,
                &f.attrs.with,
                &f.attrs.unpack_with,
                f.attrs.is_array,
            );

            quote! {
                #name: {
                    let token = tokens.next();
                    #try_unpack
                }
            }
        }
    });

    match struct_type {
        StructType::Plain => {
            quote! {
                let mut tokens = self.into_iter();

                std::result::Result::Ok(#name {
                    #(#build_fields,)*
                })
            }
        }
        StructType::Tuple => {
            quote! {
                let mut tokens = match self {
                    ::ton_abi::TokenValue::Tuple(tokens) => tokens.into_iter(),
                    _ => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
                };

                std::result::Result::Ok(#name {
                    #(#build_fields,)*
                })
            }
        }
    }
}

fn try_unpack(
    type_name: &Option<TypeName>,
    with: &Option<syn::Expr>,
    unpack_with: &Option<syn::Expr>,
    is_array: bool,
) -> proc_macro2::TokenStream {
    if let Some(type_name) = type_name.as_ref() {
        let handler = get_handler(type_name);
        match is_array {
            true => {
                quote! {
                    let array = match token {
                        Some(token) => match token.value {
                            ::ton_abi::TokenValue::Array(_, tokens) | ::ton_abi::TokenValue::FixedArray(_, tokens) => {
                                let mut vec = Vec::with_capacity(tokens.len());
                                for token in tokens {
                                    let item = match token {
                                        #handler
                                        _ => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
                                    };
                                    vec.push(item);
                                }
                                Ok(vec)
                            },
                            _ => Err(::nekoton_abi::UnpackerError::InvalidAbi),
                        }
                        None => Err(::nekoton_abi::UnpackerError::InvalidAbi),
                    };
                    array?
                }
            }
            false => {
                quote! {
                    match token {
                        Some(token) => match token.value {
                            #handler
                            _ => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
                        },
                        None => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
                    }
                }
            }
        }
    } else if let Some(with) = with.as_ref() {
        quote! {
            match token {
                Some(token) => #with::unpack(&token.value)?,
                None => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
            }
        }
    } else if let Some(unpack_with) = unpack_with.as_ref() {
        quote! {
            match token {
                Some(token) => #unpack_with(&token.value)?,
                None => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
            }
        }
    } else {
        match is_array {
            true => {
                quote! {
                    let array = match token {
                        Some(token) => match token.value {
                            ::ton_abi::TokenValue::Array(_, tokens) | ::ton_abi::TokenValue::FixedArray(_, tokens) => tokens,
                            _ => return Err(::nekoton_abi::UnpackerError::InvalidAbi),
                        }
                        .into_iter()
                        .map(::nekoton_abi::UnpackAbi::unpack)
                        .collect(),
                        None => Err(::nekoton_abi::UnpackerError::InvalidAbi),
                    };
                    array?
                }
            }
            false => {
                quote! {
                    ::nekoton_abi::UnpackAbi::unpack(token)?
                }
            }
        }
    }
}

fn get_handler(type_name: &TypeName) -> proc_macro2::TokenStream {
    match type_name {
        TypeName::Int8 => {
            quote! {
                ::ton_abi::TokenValue::Int(::ton_abi::Int { number: value, size: 8 }) => {
                    ::nekoton_abi::num_traits::ToPrimitive::to_i8(&value)
                    .ok_or(::nekoton_abi::UnpackerError::InvalidAbi)?
                },
            }
        }
        TypeName::Uint8 => {
            quote! {
                ::ton_abi::TokenValue::Uint(::ton_abi::Uint { number: value, size: 8 }) => {
                    ::nekoton_abi::num_traits::ToPrimitive::to_u8(&value)
                    .ok_or(::nekoton_abi::UnpackerError::InvalidAbi)?
                },
            }
        }
        TypeName::Uint16 => {
            quote! {
                ::ton_abi::TokenValue::Uint(::ton_abi::Uint { number: value, size: 16 }) => {
                    ::nekoton_abi::num_traits::ToPrimitive::to_u16(&value)
                    .ok_or(::nekoton_abi::UnpackerError::InvalidAbi)?
                },
            }
        }
        TypeName::Uint32 => {
            quote! {
                ::ton_abi::TokenValue::Uint(::ton_abi::Uint { number: value, size: 32 }) => {
                    ::nekoton_abi::num_traits::ToPrimitive::to_u32(&value)
                    .ok_or(::nekoton_abi::UnpackerError::InvalidAbi)?
                },
            }
        }
        TypeName::Uint64 => {
            quote! {
                ::ton_abi::TokenValue::Uint(::ton_abi::Uint { number: value, size: 64 }) => {
                    ::nekoton_abi::num_traits::ToPrimitive::to_u64(&value)
                    .ok_or(::nekoton_abi::UnpackerError::InvalidAbi)?
                },
            }
        }
        TypeName::Uint128 => {
            quote! {
                ::ton_abi::TokenValue::Uint(::ton_abi::Uint { number: value, size: 128 }) => {
                    ::nekoton_abi::num_traits::ToPrimitive::to_u128(&value)
                    .ok_or(::nekoton_abi::UnpackerError::InvalidAbi)?
                },
            }
        }
        TypeName::Uint256 => {
            quote! {
                value @ ::ton_abi::TokenValue::Uint(::ton_abi::Uint { size: 256, .. }) => {
                    ::nekoton_abi::UnpackAbi::<::ton_types::UInt256>::unpack(value)?
                }
            }
        }
        TypeName::Grams => {
            quote! {
                ::ton_abi::TokenValue::Token(::ton_block::Grams(value)) => {
                    ::std::convert::TryFrom::try_from(value)
                    .map_err(|_| ::nekoton_abi::UnpackerError::InvalidAbi)?
                }
            }
        }
        TypeName::Address => {
            quote! {
                ::ton_abi::TokenValue::Address(::ton_block::MsgAddress::AddrStd(addr)) => {
                    ::ton_block::MsgAddressInt::AddrStd(addr)
                },
                ::ton_abi::TokenValue::Address(::ton_block::MsgAddress::AddrVar(addr)) => {
                    ::ton_block::MsgAddressInt::AddrVar(addr)
                },
            }
        }
        TypeName::Cell => {
            quote! {
                ::ton_abi::TokenValue::Cell(cell) => cell,
            }
        }
        TypeName::Bool => {
            quote! {
                ::ton_abi::TokenValue::Bool(value) => value,
            }
        }
        TypeName::String => {
            quote! {
                ::ton_abi::TokenValue::String(data) => data,
            }
        }
        TypeName::Bytes => {
            quote! {
                ton_abi::TokenValue::Bytes(bytes) => bytes,
            }
        }
        TypeName::None => unreachable!(),
    }
}
