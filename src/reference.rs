use crate::{OpenAPI, Parameter, RequestBody, Response, Schema};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A structured enum of an OpenAPI reference.
/// e.g. #/components/schemas/Account or #/components/schemas/Account/properties/name
pub enum SchemaReference {
    Schema { schema: String },
    Property { schema: String, property: String },
}

impl SchemaReference {
    pub fn from_str(reference: &str) -> Self {
        let mut ns = reference.rsplit('/');
        let name = ns.next().unwrap();
        match ns.next().unwrap() {
            "schemas" => Self::Schema {
                schema: name.to_string(),
            },
            "properties" => {
                let schema_name = ns.next().unwrap();
                Self::Property {
                    schema: schema_name.to_string(),
                    property: name.to_string(),
                }
            }
            _ => panic!("Unknown reference: {}", reference),
        }
    }
}

impl std::fmt::Display for SchemaReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaReference::Schema { schema } => write!(f, "#/components/schemas/{}", schema),
            SchemaReference::Property { schema, property } => {
                write!(f, "#/components/schemas/{}/properties/{}", schema, property)
            }
        }
    }
}

/// Exists for backwards compatibility.
pub type ReferenceOr<T> = RefOr<T>;
pub type RefOr<T> = Ref<T>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Ref<T> {
    Reference {
        #[serde(rename = "$ref")]
        reference: String,
    },
    Item(T),
}

impl<T> Ref<T> {
    pub fn ref_(r: &str) -> Self {
        Ref::Reference {
            reference: r.to_owned(),
        }
    }
    pub fn schema_ref(r: &str) -> Self {
        Ref::Reference {
            reference: format!("#/components/schemas/{}", r),
        }
    }

    pub fn boxed(self) -> Box<Ref<T>> {
        Box::new(self)
    }

    /// Converts this [RefOr] to the item inside, if it exists.
    ///
    /// The return value will be [Option::Some] if this was a [RefOr::Item] or [Option::None] if this was a [RefOr::Reference].
    ///
    /// # Examples
    ///
    /// ```
    /// # use openapiv3::RefOr;
    ///
    /// let i = RefOr::Item(1);
    /// assert_eq!(i.into_item(), Some(1));
    ///
    /// let j: RefOr<u8> = RefOr::Reference { reference: String::new() };
    /// assert_eq!(j.into_item(), None);
    /// ```
    pub fn into_item(self) -> Option<T> {
        match self {
            RefOr::Reference { .. } => None,
            RefOr::Item(i) => Some(i),
        }
    }

    /// Returns a reference to to the item inside this [RefOr], if it exists.
    ///
    /// The return value will be [Option::Some] if this was a [RefOr::Item] or [Option::None] if this was a [RefOr::Reference].
    ///
    /// # Examples
    ///
    /// ```
    /// # use openapiv3::RefOr;
    ///
    /// let i = RefOr::Item(1);
    /// assert_eq!(i.as_item(), Some(&1));
    ///
    /// let j: RefOr<u8> = RefOr::Reference { reference: String::new() };
    /// assert_eq!(j.as_item(), None);
    /// ```
    pub fn as_item(&self) -> Option<&T> {
        match self {
            RefOr::Reference { .. } => None,
            RefOr::Item(i) => Some(i),
        }
    }

    pub fn as_ref_str(&self) -> Option<&str> {
        match self {
            RefOr::Reference { reference } => Some(reference),
            RefOr::Item(_) => None,
        }
    }

    pub fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            RefOr::Reference { .. } => None,
            RefOr::Item(i) => Some(i),
        }
    }

    pub fn to_mut(&mut self) -> &mut T {
        self.as_mut().expect("Not an item")
    }
}

fn resolve_helper<'a>(
    reference: &str,
    spec: &'a OpenAPI,
    seen: &mut HashSet<String>,
) -> &'a Schema {
    if seen.contains(reference) {
        panic!("Circular reference: {}", reference);
    }
    println!("\n\n\n\n\n \t\t inside resolve_helper\n\n");
    seen.insert(reference.to_string());
    let reference = SchemaReference::from_str(&reference);
    match &reference {
        SchemaReference::Schema { ref schema } => {
            let schema_ref = spec
                .schemas
                .get(schema)
                .expect(&format!("Schema {} not found in OpenAPI spec.", schema));
            // In theory both this as_item and the one below could have continue to be references
            // but assum
            match schema_ref {
                RefOr::Reference { reference } => resolve_helper(&reference, spec, seen),
                RefOr::Item(s) => s,
            }
        }
        SchemaReference::Property {
            schema: schema_name,
            property,
        } => {
            let schema = spec.schemas.get(schema_name)
                .expect(&format!("Schema {} not found in OpenAPI spec.", schema_name))
                .as_item()
                .expect(&format!("The schema {} was used in a reference, but that schema is itself a reference to another schema.", schema_name));
            let prop_schema = schema.properties().get(property).expect(&format!(
                "Schema {} does not have property {}.",
                schema_name, property
            ));
            prop_schema.resolve(spec)
        }
    }
}

impl RefOr<Schema> {
    pub fn resolve<'a>(&'a self, spec: &'a OpenAPI) -> &'a Schema {
        println!("\n\n\n\t\tINSIDE RESOLVER\n\n\n");
        match self {
            RefOr::Reference { reference } => resolve_helper(reference, spec, &mut HashSet::new()),
            RefOr::Item(schema) => schema,
        }
    }
}

impl<T> From<T> for RefOr<T> {
    fn from(item: T) -> Self {
        RefOr::Item(item)
    }
}

impl RefOr<Parameter> {
    pub fn resolve<'a>(&'a self, spec: &'a OpenAPI) -> Result<&'a Parameter> {
        match self {
            RefOr::Reference { reference } => {
                let name = get_parameter_name(&reference)?;
                spec.parameters
                    .get(name)
                    .ok_or(anyhow!("{} not found in OpenAPI spec.", reference))?
                    .as_item()
                    .ok_or(anyhow!("{} is circular.", reference))
            }
            RefOr::Item(parameter) => Ok(parameter),
        }
    }
}

impl RefOr<Response> {
    pub fn resolve<'a>(&'a self, spec: &'a OpenAPI) -> Result<&'a Response> {
        match self {
            RefOr::Reference { reference } => {
                let name = get_response_name(&reference)?;
                spec.responses
                    .get(name)
                    .ok_or(anyhow!("{} not found in OpenAPI spec.", reference))?
                    .as_item()
                    .ok_or(anyhow!("{} is circular.", reference))
            }
            RefOr::Item(response) => Ok(response),
        }
    }
}

impl Ref<RequestBody> {
    pub fn resolve<'a>(&'a self, spec: &'a OpenAPI) -> Result<&'a RequestBody> {
        match self {
            Ref::Reference { reference } => {
                let name = get_request_body_name(&reference)?;
                spec.request_bodies
                    .get(name)
                    .ok_or(anyhow!("{} not found in OpenAPI spec.", reference))?
                    .as_item()
                    .ok_or(anyhow!("{} is circular.", reference))
            }
            Ref::Item(request_body) => Ok(request_body),
        }
    }
}

impl<T: Default> Default for RefOr<T> {
    fn default() -> Self {
        Ref::Item(T::default())
    }
}

fn parse_reference<'a>(reference: &'a str, group: &str) -> Result<&'a str> {
    let mut parts = reference.rsplitn(2, '/');
    let name = parts.next();
    name.filter(|_| matches!(parts.next(), Some(x) if format!("#/components/{group}") == x))
        .ok_or(anyhow!("Invalid {} reference: {}", group, reference))
}

fn get_response_name(reference: &str) -> Result<&str> {
    parse_reference(reference, "responses")
}

fn get_request_body_name(reference: &str) -> Result<&str> {
    parse_reference(reference, "requestBodies")
}

fn get_parameter_name(reference: &str) -> Result<&str> {
    parse_reference(reference, "parameters")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_request_body_name() {
        assert!(matches!(
            get_request_body_name("#/components/requestBodies/Foo"),
            Ok("Foo")
        ));
        assert!(get_request_body_name("#/components/schemas/Foo").is_err());
    }
}
