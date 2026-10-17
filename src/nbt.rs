//! Tiny NBT subset for the crowbar's spawner round-trips.
//!
//! The host exchanges block-entity data as unnamed-root network NBT
//! (`Nbt::write_unnamed`: the compound tag id, no root name, then the
//! compound payload). The crowbar only has to lift the spawned mob out of a
//! broken spawner (`SpawnData.entity.id`), to build the `block_entity_data`
//! component of the extracted spawner item and to queue the pending block
//! entity NBT for its placement, so this module walks the tags it does not
//! need instead of modelling them.

const TAG_END: u8 = 0;
const TAG_INT: u8 = 3;
const TAG_STRING: u8 = 8;
const TAG_COMPOUND: u8 = 10;

/// One decoded tag: compounds and strings are kept, everything else is only
/// walked over.
enum Value {
    Compound(Vec<(String, Value)>),
    Text(String),
    Other,
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(len)?;
        let slice = self.data.get(self.offset..end)?;
        self.offset = end;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(*self.take(1)?.first()?)
    }

    fn u16(&mut self) -> Option<u16> {
        let bytes = self.take(2)?;
        Some(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i32(&mut self) -> Option<i32> {
        let bytes = self.take(4)?;
        Some(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn string(&mut self) -> Option<String> {
        let len = self.u16()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    /// Reads the payload of one `tag`, walking over the types we do not keep.
    fn value(&mut self, tag: u8) -> Option<Value> {
        Some(match tag {
            1 => {
                self.take(1)?;
                Value::Other
            } // byte
            2 => {
                self.take(2)?;
                Value::Other
            } // short
            3 => {
                self.take(4)?;
                Value::Other
            } // int
            4 => {
                self.take(8)?;
                Value::Other
            } // long
            5 => {
                self.take(4)?;
                Value::Other
            } // float
            6 => {
                self.take(8)?;
                Value::Other
            } // double
            7 => {
                // byte array
                let len = self.i32()?.max(0) as usize;
                self.take(len)?;
                Value::Other
            }
            8 => Value::Text(self.string()?),
            9 => {
                // list: element tag id, then that many element payloads
                let element = self.u8()?;
                for _ in 0..self.i32()?.max(0) {
                    self.value(element)?;
                }
                Value::Other
            }
            10 => Value::Compound(self.compound()?),
            11 => {
                // int array
                let len = self.i32()?.max(0) as usize;
                self.take(len.checked_mul(4)?)?;
                Value::Other
            }
            12 => {
                // long array
                let len = self.i32()?.max(0) as usize;
                self.take(len.checked_mul(8)?)?;
                Value::Other
            }
            _ => return None,
        })
    }

    /// Reads a compound payload: named entries until `TAG_END`.
    fn compound(&mut self) -> Option<Vec<(String, Value)>> {
        let mut entries = Vec::new();

        loop {
            let tag = self.u8()?;
            if tag == TAG_END {
                return Some(entries);
            }

            let name = self.string()?;
            entries.push((name, self.value(tag)?));
        }
    }
}

/// Walks the compound keys of `path` and returns the string found there.
pub fn find_string(bytes: &[u8], path: &[&str]) -> Option<String> {
    let mut reader = Reader::new(bytes);
    if reader.u8()? != TAG_COMPOUND {
        return None;
    }

    let mut current = Value::Compound(reader.compound()?);

    for key in path {
        let Value::Compound(entries) = current else {
            return None;
        };

        current = entries
            .into_iter()
            .find(|(name, _)| name.as_str() == *key)
            .map(|(_, value)| value)?;
    }

    match current {
        Value::Text(text) => Some(text),
        _ => None,
    }
}

fn push_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn push_named(out: &mut Vec<u8>, tag: u8, name: &str) {
    out.push(tag);
    push_string(out, name);
}

/// `SpawnData: { entity: { id: <mob> } }` (the 1.20.5+ spawner layout).
fn push_spawn_data(out: &mut Vec<u8>, mob: &str) {
    push_named(out, TAG_COMPOUND, "SpawnData");
    push_named(out, TAG_COMPOUND, "entity");
    push_named(out, TAG_STRING, "id");
    push_string(out, mob);
    out.push(TAG_END);
    out.push(TAG_END);
}

/// `block_entity_data` component payload of an extracted spawner: the unnamed
/// compound the item component codec reads (behind the tag id VarInt).
pub fn write_spawner_item_data(mob: &str) -> Vec<u8> {
    let mut out = vec![TAG_COMPOUND];
    push_spawn_data(&mut out, mob);
    out.push(TAG_END);
    out
}

/// Pending block-entity NBT of an extracted spawner for `set_block_entity_nbt`
/// (the `write_unnamed` layout): the spawner data plus the identity and
/// position `block_entity_from_nbt` keys off.
pub fn write_spawner_pending(mob: &str, x: i32, y: i32, z: i32) -> Vec<u8> {
    let mut out = vec![TAG_COMPOUND];

    push_named(&mut out, TAG_STRING, "id");
    push_string(&mut out, "minecraft:mob_spawner");

    for (name, value) in [("x", x), ("y", y), ("z", z)] {
        push_named(&mut out, TAG_INT, name);
        out.extend_from_slice(&value.to_be_bytes());
    }

    push_spawn_data(&mut out, mob);
    out.push(TAG_END);
    out
}
