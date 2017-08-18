use std::collections::HashMap;
use serde::Deserialize;
use super::*;

static BAD_DATA: &'static str = "Bad data provided";

#[test]
fn reads_pod_xml_value() {
    let data = r#"<?xml version="1.0"?><string>South Dakota</string>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::String("South Dakota".into()));
    let data = r#"<?xml version="1.0"?><string />"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::String("".into()));
    let data = r#"<?xml version="1.0"?><string></string>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::String("".into()));

    let data = r#"<?xml version="1.0"?><int>-33</int>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Int(-33));
    let data = r#"<?xml version="1.0"?><i4>-33</i4>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Int(-33));

    let data = r#"<?xml version="1.0"?><boolean>1</boolean>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Bool(true));
    let data = r#"<?xml version="1.0"?><boolean>0</boolean>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Bool(false));

    let data = r#"<?xml version="1.0"?><double>-44.2</double>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Double(-44.2));

    let data = r#"<?xml version="1.0"?><dateTime.iso8601>33</dateTime.iso8601>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::DateTime("33".into()));

    let data = r#"<?xml version="1.0"?><base64>ASDF=</base64>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Base64("ASDF=".into()));
}

#[test]
fn reads_array_xml_value() {
    let data = r#"<?xml version="1.0"?>
<array>
    <data>
        <value><i4>33</i4></value>
        <value><i4>-12</i4></value>
        <value><i4>44</i4></value>
    </data>
</array>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(
        data,
        Value::Array(vec![Value::Int(33), Value::Int(-12), Value::Int(44)])
    );
}

#[test]
fn reads_struct_xml_value() {
    let mut fields = HashMap::<String, Value>::new();
    fields.insert("foo".into(), Value::Int(42));
    fields.insert("bar".into(), Value::String("baz".into()));
    let data = r#"<?xml version="1.0"?>
<struct>
    <member>
        <name>foo</name>
        <value><i4>42</i4></value>
    </member>
    <member>
        <name>bar</name>
        <value><string>baz</string></value>
    </member>
</struct>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, Value::Struct(fields));
}

#[test]
fn reads_response() {
    let mut fields = HashMap::<String, Value>::new();
    fields.insert("foo".into(), Value::Int(42));
    fields.insert("bar".into(), Value::String("baz".into()));
    let params = vec![Value::String("South Dakota".into()), Value::Struct(fields)];
    let data = r#"<?xml version="1.0"?>
<methodResponse>
    <params>
        <param>
            <value><string>South Dakota</string></value>
        </param>
        <param>
            <value>
                <struct>
                    <member>
                        <name>foo</name>
                        <value><i4>42</i4></value>
                    </member>
                    <member>
                        <name>bar</name>
                        <value><string>baz</string></value>
                    </member>
                </struct>
            </value>
        </param>
    </params>
</methodResponse>"#;
    let data = parse::response_value(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data, ResponseValue::Success { params: params });
}

#[test]
fn reads_fault() {
    let data = r#"<?xml version="1.0"?>
<methodResponse>
    <fault>
        <value>
            <struct>
                <member>
                    <name>faultCode</name>
                    <value><int>4</int></value>
                </member>
                <member>
                    <name>faultString</name>
                    <value><string>Too many parameters.</string></value>
                </member>
            </struct>
        </value>
    </fault>
</methodResponse>"#;
    let data = parse::response_value(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(
        data,
        ResponseValue::Fault {
            code: 4,
            message: "Too many parameters.".into(),
        }
    );
}


#[test]
fn reads_call() {
    let mut fields = HashMap::<String, Value>::new();
    fields.insert("foo".into(), Value::Int(42));
    fields.insert("bar".into(), Value::String("baz".into()));
    let data = r#"<?xml version="1.0"?>
<methodCall>
    <methodName>foobar</methodName>
    <params>
        <param>
            <value><string>South Dakota</string></value>
        </param>
        <param>
            <value>
                <struct>
                    <member>
                        <name>foo</name>
                        <value><i4>42</i4></value>
                    </member>
                    <member>
                        <name>bar</name>
                        <value><string>baz</string></value>
                    </member>
                </struct>
            </value>
        </param>
    </params>
</methodCall>"#;
    let data = parse::call_value(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(data.name, String::from("foobar"));
    assert_eq!(
        data.params,
        vec![Value::String("South Dakota".into()), Value::Struct(fields)]
    );
}

#[derive(Debug, PartialEq, Deserialize)]
struct TestStruct {
    foo: i32,
    bar: String,
}

#[derive(Debug, PartialEq, Deserialize)]
struct TestStructTuple(String, TestStruct);

#[test]
fn reads_array_structure_xml_value() {
    let data = r#"<?xml version="1.0"?>
<array>
    <data>
        <value><i4>33</i4></value>
        <value><i4>-12</i4></value>
        <value><i4>44</i4></value>
    </data>
</array>"#;
    let data = parse::xml(data.as_bytes()).expect(BAD_DATA);
    let data = Vec::<i32>::deserialize(data).expect(BAD_DATA);
    assert_eq!(data, vec![33, -12, 44]);
}

#[test]
fn reads_response_structure() {
    let data = r#"<?xml version="1.0"?>
<methodResponse>
    <params>
        <param>
            <value><string>South Dakota</string></value>
        </param>
        <param>
            <value>
                <struct>
                    <member>
                        <name>foo</name>
                        <value><i4>42</i4></value>
                    </member>
                    <member>
                        <name>bar</name>
                        <value><string>baz</string></value>
                    </member>
                </struct>
            </value>
        </param>
    </params>
</methodResponse>"#;
    let resp: Response<TestStructTuple> = parse::response(data.as_bytes()).expect(BAD_DATA);
    let data = match resp {
        Response::Success(v) => v,
        _ => panic!("Unespected faulty response!"),
    };
    assert_eq!(data.0, String::from("South Dakota"));
    assert_eq!(data.1.foo, 42);
    assert_eq!(data.1.bar, String::from("baz"));
}

#[test]
fn reads_call_structure() {
    let data = r#"<?xml version="1.0"?>
<methodCall>
    <methodName>foobar</methodName>
    <params>
        <param>
            <value><string>South Dakota</string></value>
        </param>
        <param>
            <value>
                <struct>
                    <member>
                        <name>foo</name>
                        <value><i4>42</i4></value>
                    </member>
                    <member>
                        <name>bar</name>
                        <value><string>baz</string></value>
                    </member>
                </struct>
            </value>
        </param>
    </params>
</methodCall>"#;
    let Call::<TestStructTuple> { name, data } = parse::call(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(name, String::from("foobar"));
    assert_eq!(data.0, String::from("South Dakota"));
    assert_eq!(data.1.foo, 42);
    assert_eq!(data.1.bar, String::from("baz"));
}

fn ser_and_de(value: Value) {
    ser_and_de_response_value(ResponseValue::Success { params: vec![value] });
}

fn ser_and_de_call_value(value: CallValue) {
    let data = format!("{}", value);
    let data = parse::call_value(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(value, data);
}

fn ser_and_de_response_value(value: ResponseValue) {
    let data = format!("{}", value);
    let data = parse::response_value(data.as_bytes()).expect(BAD_DATA);
    assert_eq!(value, data);
}

#[test]
fn writes_pod_xml_value() {
    ser_and_de(Value::String("South Dakota".into()));
    ser_and_de(Value::String("".into()));
    ser_and_de(Value::String("".into()));
    ser_and_de(Value::Int(-33));
    ser_and_de(Value::Int(-33));
    ser_and_de(Value::Bool(true));
    ser_and_de(Value::Bool(false));
    ser_and_de(Value::Double(-44.2));
    ser_and_de(Value::DateTime("33".into()));
    ser_and_de(Value::Base64("ASDF=".into()));
}

#[test]
fn writes_array_xml_value() {
    ser_and_de(Value::Array(
        vec![Value::Int(33), Value::Int(-12), Value::Int(44)],
    ));
}

#[test]
fn writes_struct_xml_value() {
    let mut fields = HashMap::<String, Value>::new();
    fields.insert("foo".into(), Value::Int(42));
    fields.insert("bar".into(), Value::String("baz".into()));
    ser_and_de(Value::Struct(fields));
}

#[test]
fn writes_response() {
    let mut fields = HashMap::<String, Value>::new();
    fields.insert("foo".into(), Value::Int(42));
    fields.insert("bar".into(), Value::String("baz".into()));
    let params = vec![Value::String("South Dakota".into()), Value::Struct(fields)];
    ser_and_de_response_value(ResponseValue::Success { params: params })
}

#[test]
fn writes_fault() {
    ser_and_de_response_value(ResponseValue::Fault {
        code: 4,
        message: "Too many parameters.".into(),
    });
}


#[test]
fn writes_call() {
    let mut fields = HashMap::<String, Value>::new();
    fields.insert("foo".into(), Value::Int(42));
    fields.insert("bar".into(), Value::String("baz".into()));
    ser_and_de_call_value(CallValue {
        name: String::from("foobar"),
        params: vec![Value::String("South Dakota".into()), Value::Struct(fields)],
    });
}