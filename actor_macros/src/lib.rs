use proc_macro::TokenStream;
use quote::format_ident;
use quote::quote;
use quote::ToTokens;

use syn::parse_macro_input;

#[proc_macro_attribute]
pub fn threaded_actor(args: TokenStream, input: TokenStream) -> TokenStream {
    eprintln!("Args: {}", args);
    eprintln!("Input: {}", input);

    let _ = args;
    let ast = parse_macro_input!(input as syn::ItemImpl);

    let input_clone = ast.clone();

    let this_actor = *ast.self_ty.clone();
    let this_actor_string = this_actor.to_token_stream().to_string();
    let this_actor_req = format_ident!("{}Req", this_actor_string);
    let this_actor_res = format_ident!("{}Res", this_actor_string);
    let this_actor_ref = format_ident!("{}Ref", this_actor_string);
    let this_actor_ref_poll = format_ident!("{}RefPoll", this_actor_string);

    eprintln!("Self {}", this_actor.to_token_stream());

    let mut request_list = Vec::new();
    let mut response_list = Vec::new();
    let mut block_api_fn_list = Vec::new();
    let mut poll_api_fn_list = Vec::new();
    let mut handler_match_block_list = Vec::new();
    for item in ast.items {
        eprintln!("Token: {}", item.to_token_stream());
        match item {
            syn::ImplItem::Fn(fns) => {
                let sig = fns.sig;
                let function_name = sig.ident;

                // eprintln!("Fn: {}", sig.inputs.to_token_stream());
                // TODO, Panic on `owned self` APIs

                let mut inputs = Vec::new();
                let mut input_args = Vec::new();
                let mut input_full_args = Vec::new();
                sig.inputs.iter().for_each(|m| match m {
                    syn::FnArg::Typed(pat_type) => {
                        let pat = &pat_type.pat;
                        let ty = &pat_type.ty;
                        inputs.push(quote! {
                            #ty,
                        });
                        input_args.push(quote! {
                            #pat,
                        });
                        input_full_args.push(quote! {
                            #pat_type,
                        });
                    }
                    syn::FnArg::Receiver(_) => {}
                });
                request_list.push(quote! {
                    #function_name(#(#inputs)*),
                });

                // eprintln!("OutputFn: {}", sig.output.to_token_stream());
                let output = match sig.output {
                    syn::ReturnType::Default => {
                        quote! {
                            ()
                        }
                    }
                    syn::ReturnType::Type(_, ty) => {
                        quote! {
                            #ty
                        }
                    }
                };
                response_list.push(quote! {
                    #function_name(#output),
                });

                // Block API
                let function_name_as_poll = format_ident!("{}_as_poll", function_name);
                let block_api_fn = quote! {
                    pub fn #function_name(&self, #(#input_full_args)*) -> Result<#output, threaded_actor::ActorError> {
                        let response = self.actor_ref.block(#this_actor_req::#function_name(#(#input_args)*))?;
                        let data = match response {
                            #this_actor_res::#function_name(d) => d,
                            _ => unreachable!(),
                        };
                        Ok(data)
                    }

                    // pub fn #function_name_as_poll(&self, #(#input_full_args)*) -> threaded_actor::ActorRefPollPromise<#this_actor_req, #this_actor_res> {
                    //     self.actor_ref.as_poll(#this_actor_req::#function_name(#(#input_args)*))
                    // }
                };
                block_api_fn_list.push(block_api_fn);

                // Poll API
                let poll_api_fn = quote! {
                    pub fn #function_name(&self, on_req: impl Fn() -> (#(#inputs)*)) -> #output {
                        // let res = self.actor_ref.poll_once(|| {
                        //     let data = on_req();
                        //     MyActorReq::ping()
                        // })?;
                        todo!()
                    }
                };
                poll_api_fn_list.push(poll_api_fn);

                let handler_match_block = quote! {
                    #this_actor_req::#function_name(#(#input_args)*) => {
                        let temp = self.#function_name(#(#input_args)*);
                        #this_actor_res::#function_name(temp)
                    },
                };
                handler_match_block_list.push(handler_match_block);
            }
            _ => panic!("Not allowed by macro: {}", item.to_token_stream()),
        }
    }

    let actor_req_enum = quote! {
        pub enum #this_actor_req {
            #(#request_list)*
        }
    };
    let actor_res_enum = quote! {
        pub enum #this_actor_res {
            #(#response_list)*
        }
    };
    let actor_ref_struct = quote! {
        pub struct #this_actor_ref {
            actor_ref: threaded_actor::ActorRef<#this_actor_req, #this_actor_res>
        }

        impl From<threaded_actor::ActorRef<#this_actor_req, #this_actor_res>> for #this_actor_ref {
            fn from(actor_ref: threaded_actor::ActorRef<#this_actor_req, #this_actor_res>) -> Self {
                #this_actor_ref { actor_ref }
            }
        }

        impl Clone for #this_actor_ref {
            fn clone(&self) -> Self {
                Self { actor_ref: self.actor_ref.clone() }
            }
        }

        impl #this_actor_ref {
            #(#block_api_fn_list)*
        }
    };
    // TODO, Handle PollPromise properly
    let actor_ref_polled_struct = quote! {
        pub struct #this_actor_ref_poll {
            actor_ref: threaded_actor::ActorRefPoll<#this_actor_req, #this_actor_res>
        }

        impl Clone for #this_actor_ref_poll {
            fn clone(&self) -> Self {
                Self { actor_ref: self.actor_ref.clone() }
            }
        }

        impl #this_actor_ref_poll {
            #(#poll_api_fn_list)*
        }
    };
    let actor_ref_handler = quote! {
        impl threaded_actor::ActorHandler<#this_actor_req, #this_actor_res> for #this_actor {
            fn handle(&mut self, request: #this_actor_req) -> #this_actor_res {
                match request {
                    #(#handler_match_block_list)*
                }
            }
        }
    };

    // #actor_ref_polled_struct
    let output = quote! {
        #input_clone

        #actor_req_enum

        #actor_res_enum

        #actor_ref_handler

        #actor_ref_struct
    };

    // TODO, Remove this later
    eprintln!("---------------");
    // eprintln!("{}", output);
    eprintln!("---------------");
    let file = syn::parse_file(&output.to_string()).unwrap();
    eprintln!("{}", prettyplease::unparse(&file));

    TokenStream::from(output)
}
