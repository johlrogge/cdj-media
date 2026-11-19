//! Index pages: the first page of every table (slice 7d).
//!
//! An index page lists the table's data pages that hold deleted rows. A fresh file has none,
//! so the writer only builds empty index pages. See [`build_index_page`] for the layout.

use super::super::Page;
use super::super::page::{IndexBody, PageHeader};
use super::PageLinks;
use crate::bytes::Writer;
use crate::error::EncodeError;

/// `page_flags` of an index page: on all 20 real index pages.
pub(crate) const INDEX_PAGE_FLAGS: u8 = 0x64;
/// The page number that means "no page" in an index page: 26 bits set.
pub(crate) const NO_PAGE: u32 = 0x03ff_ffff;
/// An empty index entry: [`NO_PAGE`] shifted past the 3 flag bits, flags 0.
pub(crate) const EMPTY_ENTRY: u32 = NO_PAGE << 3;
/// The `u16` value rekordbox stores where there is nothing to record, as in a data page's
/// transaction fields without a transaction.
const NONE_U16: u16 = 0x1fff;
/// Length of the index header after the common page header, before the entries.
const INDEX_HEADER_LEN: usize = 0x1c;
/// Zero bytes after the entries, at the end of the page.
const INDEX_TAIL_LEN: usize = 0x14;
/// Offset of the index header: after the common page header.
const BODY_START: usize = 0x20;

/// Builds the empty index page of a table, `len_page` bytes long: the page rekordbox
/// writes for a table without deleted rows.
///
/// Common header: `links`, type `table_type`, flags [`INDEX_PAGE_FLAGS`], every other field
/// 0. After it (offsets from 0x20):
///
/// | bytes      | value                                                              |
/// |------------|--------------------------------------------------------------------|
/// | 0x00..0x04 | two `u16` 0x1fff                                                   |
/// | 0x04..0x06 | the number of entry slots: what the page leaves for them, ÷ 4      |
/// | 0x06..0x08 | 0 (1 to 169 on real pages with entries)                            |
/// | 0x08..0x0c | the page's own index                                               |
/// | 0x0c..0x10 | the table's first data page, or [`NO_PAGE`] when it has none       |
/// | 0x10..0x14 | [`NO_PAGE`]                                                        |
/// | 0x14..0x18 | 0                                                                  |
/// | 0x18..0x1a | the number of entries: 0                                           |
/// | 0x1a..0x1c | 0x1fff                                                             |
/// | 0x1c..     | the entry slots, each [`EMPTY_ENTRY`], then 0x14 zero bytes        |
///
/// # Errors
///
/// [`EncodeError::LengthOverflow`] (`"pdb index page"`) when the page is too short for the
/// index header, or holds more slots than a `u16` counts.
pub(crate) fn build_index_page(
    len_page: usize,
    table_type: u32,
    links: PageLinks,
    first_data_page: Option<u32>,
) -> Result<Page, EncodeError> {
    let slots = len_page
        .checked_sub(BODY_START + INDEX_HEADER_LEN + INDEX_TAIL_LEN)
        .map(|room| room / 4)
        .and_then(|slots| u16::try_from(slots).ok())
        .ok_or(EncodeError::LengthOverflow {
            context: "pdb index page",
        })?;
    let mut w = Writer::new();
    w.u16_le(NONE_U16);
    w.u16_le(NONE_U16);
    w.u16_le(slots);
    w.u16_le(0);
    w.u32_le(links.page_index);
    w.u32_le(first_data_page.unwrap_or(NO_PAGE));
    w.u32_le(NO_PAGE);
    w.u32_le(0);
    w.u16_le(0);
    w.u16_le(NONE_U16);
    for _ in 0..slots {
        w.u32_le(EMPTY_ENTRY);
    }
    let mut body = w.into_inner();
    body.resize(len_page - BODY_START, 0);
    let header = PageHeader {
        page_index: links.page_index,
        page_type: table_type,
        next_page: links.next_page,
        sequence: links.sequence,
        page_flags: INDEX_PAGE_FLAGS,
        free_size: 0,
        used_size: 0,
    };
    Ok(Page::index(header, IndexBody::new(body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINKS: PageLinks = PageLinks {
        page_index: 9,
        next_page: 10,
        sequence: 1,
    };

    fn expected(len: usize, first_data: u32, slots: u16) -> Vec<u8> {
        let mut out = Vec::new();
        for value in [0u32, 9, 4, 10, 1, 0, 0x6400_0000, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0x1fffu16, 0x1fff, slots, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [9u32, first_data, 0x03ff_ffff, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0, 0xff, 0x1f]);
        for _ in 0..slots {
            out.extend_from_slice(&0x1fff_fff8u32.to_le_bytes());
        }
        out.extend_from_slice(&[0; 0x14]);
        assert_eq!(out.len(), len);
        out
    }

    #[test]
    fn an_empty_index_page_is_laid_out_as_rekordbox_does() {
        let page = build_index_page(512, 4, LINKS, Some(10)).unwrap();
        // 512 − 0x20 − 0x1c − 0x14 = 432 bytes: 108 slots.
        let bytes = expected(512, 10, 108);
        assert_eq!(page.to_bytes().unwrap(), bytes);
        assert_eq!(Page::decode(&bytes, 0).unwrap(), page);
        assert_eq!(page.page_flags(), INDEX_PAGE_FLAGS);
    }

    #[test]
    fn an_empty_table_has_no_first_data_page() {
        let page = build_index_page(4096, 4, LINKS, None).unwrap();
        // 1004 slots, as on every real index page.
        assert_eq!(page.to_bytes().unwrap(), expected(4096, NO_PAGE, 1004));
    }

    #[test]
    fn pages_too_short_or_too_long_fail() {
        let overflow = Err(EncodeError::LengthOverflow {
            context: "pdb index page",
        });
        assert_eq!(build_index_page(0x4f, 4, LINKS, None), overflow);
        assert!(build_index_page(0x50, 4, LINKS, None).is_ok());
        assert_eq!(
            build_index_page(0x50 + 4 * 0x10000, 4, LINKS, None),
            overflow
        );
    }
}
