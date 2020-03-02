use ogg::reading::{PacketReader, PageParser};
use ogg::writing::PacketWriteEndInfo;
use ogg::writing::PacketWriter;
use rustc_hex::ToHex;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::io::{BufReader, BufWriter};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Corner, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, List, Paragraph, Text};
use tui::Terminal;

use crate::util::{
    event::{Event, Events},
    StatefulList,
};

mod util;

#[derive(Debug, Copy, Clone, PartialEq)]
enum BareOggFormat {
    Vorbis,
    Opus,
    Theora,
    Speex,
    Skeleton,
}

/// See https://github.com/est31/ogg-metadata/blob/b61e5f28530b5d461b98cb0167e8a561af436ebd/src/lib.rs#L154
fn identify_packet_data_by_magic(pck_data: &[u8]) -> Option<(usize, BareOggFormat)> {
    // Magic sequences.
    // https://www.xiph.org/vorbis/doc/Vorbis_I_spec.html#x1-620004.2.1
    let vorbis_magic = &[0x01, 0x76, 0x6f, 0x72, 0x62, 0x69, 0x73];
    // https://tools.ietf.org/html/rfc7845#section-5.1
    let opus_magic = &[0x4f, 0x70, 0x75, 0x73, 0x48, 0x65, 0x61, 0x64];
    // https://www.theora.org/doc/Theora.pdf#section.6.2
    let theora_magic = &[0x80, 0x74, 0x68, 0x65, 0x6f, 0x72, 0x61];
    // http://www.speex.org/docs/manual/speex-manual/node8.html
    let speex_magic = &[0x53, 0x70, 0x65, 0x65, 0x78, 0x20, 0x20, 0x20];
    // https://wiki.xiph.org/Ogg_Skeleton_4#Ogg_Skeleton_version_4.0_Format_Specification
    let skeleton_magic = &[0x66, 105, 115, 104, 101, 97, 100, 0];

    if pck_data.len() < 1 {
        return None;
    }

    use BareOggFormat::*;
    let ret: (usize, BareOggFormat) = match pck_data[0] {
        0x01 if pck_data.starts_with(vorbis_magic) => (vorbis_magic.len(), Vorbis),
        0x4f if pck_data.starts_with(opus_magic) => (opus_magic.len(), Opus),
        0x80 if pck_data.starts_with(theora_magic) => (theora_magic.len(), Theora),
        0x53 if pck_data.starts_with(speex_magic) => (speex_magic.len(), Speex),
        0x66 if pck_data.starts_with(skeleton_magic) => (speex_magic.len(), Skeleton),

        _ => return None,
    };

    return Some(ret);
}

fn select_bitstream_with_video(
    bitstreams: HashMap<u32, Vec<ogg::Packet>>,
) -> Option<Vec<ogg::Packet>> {
    let mut select_bitstream = None;
    for (_, bitstream) in bitstreams {
        // let mut packet_reader = BufReader::new(std::io::Cursor::new(&bitstream[0].data));
        let format = identify_packet_data_by_magic(&bitstream[0].data);
        match format {
            Some((_, BareOggFormat::Theora)) => select_bitstream = Some(bitstream),
            _ => {}
        }
    }
    select_bitstream
}

struct PageHeader {
    bytes: [u8; 27],
}

impl PageHeader {
    pub fn display_text(&self) -> String {
        let stream_serial = self.bitstream_serial_number().to_hex::<String>();
        let stream_page = self.page_sequence_number_parsed();

        format!(
            "Page Header - stream serial: {} - page: {}",
            stream_serial, stream_page
        )
    }

    pub fn parser(&self) -> Result<(PageParser, usize), ogg::OggReadError> {
        PageParser::new(self.bytes)
    }

    pub fn byte_display_text(&self) -> Vec<Text> {
        vec![
            Text::styled(
                self.capture_pattern().to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Red),
            ),
            Text::raw("\n"),
            Text::styled(
                self.version().to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::LightRed),
            ),
            Text::styled(
                self.header_type().to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Yellow),
            ),
            Text::styled(
                self.granule_position()[0..2].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Green),
            ),
            Text::raw("\n"),
            Text::styled(
                self.granule_position()[2..6].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Green),
            ),
            Text::raw("\n"),
            Text::styled(
                self.granule_position()[6..8].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Green),
            ),
            Text::styled(
                self.bitstream_serial_number()[0..2].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Blue),
            ),
            Text::raw("\n"),
            Text::styled(
                self.bitstream_serial_number()[2..4].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Blue),
            ),
            Text::styled(
                self.page_sequence_number()[0..2].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Cyan),
            ),
            Text::raw("\n"),
            Text::styled(
                self.page_sequence_number()[2..4].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::Cyan),
            ),
            Text::styled(
                self.checksum()[0..2].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::LightGreen),
            ),
            Text::raw("\n"),
            Text::styled(
                self.checksum()[2..4].to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::LightGreen),
            ),
            Text::styled(
                self.page_segments().to_hex::<String>(),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Text::raw("  "),
        ]
    }

    pub fn page_segments_count(&self) -> u8 {
        self.page_segments()[0]
    }

    pub fn page_sequence_number_parsed(&self) -> u32 {
        let input = self.page_sequence_number();
        let (int_bytes, _) = input.split_at(std::mem::size_of::<u32>());
        // *input = rest;
        u32::from_le_bytes(int_bytes.try_into().unwrap())
    }

    pub fn capture_pattern(&self) -> &[u8] {
        &self.bytes[0..4]
    }

    pub fn version(&self) -> &[u8] {
        &self.bytes[4..5]
    }

    pub fn header_type(&self) -> &[u8] {
        &self.bytes[5..6]
    }

    pub fn granule_position(&self) -> &[u8] {
        &self.bytes[6..14]
    }

    pub fn bitstream_serial_number(&self) -> &[u8] {
        &self.bytes[14..18]
    }

    pub fn page_sequence_number(&self) -> &[u8] {
        &self.bytes[18..22]
    }

    pub fn checksum(&self) -> &[u8] {
        &self.bytes[22..26]
    }

    pub fn page_segments(&self) -> &[u8] {
        &self.bytes[26..27]
    }
}

fn get_packets() -> Vec<PageHeader> {
    let in_file_path = std::env::args()
        .nth(1)
        .expect("Missing input video argument");

    let mut in_video_meta = std::fs::metadata(in_file_path.clone()).unwrap();
    let mut in_video = File::open(in_file_path).unwrap();

    // dbg!(ogg_metadata::read_format(&mut in_video));
    // dbg!(ogg_metadata::read_format(&mut in_video));
    // dbg!(ogg_metadata::read_format(&mut in_video));
    // dbg!(ogg_metadata::read_format(in_audio));

    // let mut in_video_logical_bitstreams = HashMap::new();
    // let mut in_video_reader = PacketReader::new(BufReader::new(in_video));
    // while let Some(packet) = in_video_reader.read_packet().unwrap() {
    // in_video_logical_bitstreams
    // .entry(packet.stream_serial())
    // .or_insert(Vec::new())
    // .push(packet);
    // }

    let mut next_header_start = 0;
    let mut headers = Vec::new();
    while in_video_meta.len() != next_header_start {
        let mut header_bytes: [u8; 27] = [0; 27];
        in_video.seek(SeekFrom::Start(next_header_start)).unwrap();
        in_video.read_exact(&mut header_bytes).unwrap();
        let header = PageHeader {
            bytes: header_bytes,
        };
        let segments_lengths: u64 = (0..header.page_segments_count())
            .into_iter()
            .map(|_| [0u8; 1])
            .map(|mut segment_length| {
                in_video.read_exact(&mut segment_length).unwrap();
                segment_length
            })
            .map(|segment_length| segment_length[0] as u64)
            .sum();

        next_header_start += 27u64 + header.page_segments_count() as u64 + segments_lengths;
        headers.push(header);
    }

    headers

    // let second_header_start =
    // 27u64 + first_header.page_segments_count() as u64 + first_segments_lengths;
    // dbg!(second_header_start);
    // let mut second_header_bytes: [u8; 27] = [0; 27];
    // in_video.seek(SeekFrom::Start(second_header_start)).unwrap();
    // in_video.read_exact(&mut second_header_bytes).unwrap();
    // let second_header = PageHeader {
    // bytes: second_header_bytes,
    // };
    // let second_segments_lengths: u64 = (0..second_header.page_segments_count())
    // .into_iter()
    // .map(|_| [0u8; 1])
    // .map(|mut segment_length| {
    // in_video.read_exact(&mut segment_length).unwrap();
    // segment_length
    // })
    // .map(|segment_length| segment_length[0] as u64)
    // .sum();

    // let third_header_start = 27u64
    // + first_header.page_segments_count() as u64
    // + first_segments_lengths
    // + 27u64
    // + second_header.page_segments_count() as u64
    // + second_segments_lengths;
    // let mut third_header_bytes: [u8; 27] = [0; 27];
    // in_video.seek(SeekFrom::Start(third_header_start)).unwrap();
    // in_video.read_exact(&mut third_header_bytes).unwrap();
    // let third_header = PageHeader {
    // bytes: third_header_bytes,
    // };
    // let third_segments_lengths: u64 = (0..third_header.page_segments_count())
    // .into_iter()
    // .map(|_| [0u8; 1])
    // .map(|mut segment_length| {
    // in_video.read_exact(&mut segment_length).unwrap();
    // segment_length
    // })
    // .map(|segment_length| segment_length[0] as u64)
    // .sum();

    // // dbg!(segments_lengths);

    // vec![first_header, second_header, third_header]
}

struct App {
    page_headers: StatefulList<PageHeader>,
}

impl App {
    fn new(page_headers: Vec<PageHeader>) -> App {
        App {
            page_headers: StatefulList::with_items(page_headers),
        }
    }
}

fn main() -> Result<(), failure::Error> {
    let page_headers = get_packets();
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let events = Events::new();

    // App
    let mut app = App::new(page_headers);

    loop {
        terminal.draw(|mut f| {
            let lr_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(f.size());

            let style = Style::default().fg(Color::White).bg(Color::Black);

            let items = app
                .page_headers
                .items
                .iter()
                .map(|i| Text::raw(i.display_text()));
            let items = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("List"))
                .style(style)
                .highlight_style(style.fg(Color::LightGreen).modifier(Modifier::BOLD))
                .highlight_symbol(">");
            f.render_stateful_widget(items, lr_chunks[0], &mut app.page_headers.state);

            let selected_page_header = {
                app.page_headers
                    .state
                    .selected()
                    .map(|item_index| app.page_headers.items.get(item_index).unwrap())
            };
            if let Some(selected_page_header) = selected_page_header {
                let text = selected_page_header.byte_display_text();
                let byte_display = Paragraph::new(text.iter())
                    .block(Block::default().title("Header bytes").borders(Borders::ALL))
                    .style(Style::default().fg(Color::White).bg(Color::Black))
                    .alignment(Alignment::Center)
                    .wrap(true);
                f.render_widget(byte_display, lr_chunks[1]);
            }

            // let events = app.events.iter().map(|&(evt, level)| {
            // Text::styled(
            // format!("{}: {}", level, evt),
            // match level {
            // "ERROR" => app.error_style,
            // "CRITICAL" => app.critical_style,
            // "WARNING" => app.warning_style,
            // _ => app.info_style,
            // },
            // )
            // });
            // let events_list = List::new(events)
            // .block(Block::default().borders(Borders::ALL).title("List"))
            // .start_corner(Corner::BottomLeft);
            // f.render_widget(events_list, chunks[1]);
        })?;

        match events.next()? {
            Event::Input(input) => match input {
                Key::Char('q') => {
                    break;
                }
                Key::Left => {
                    app.page_headers.unselect();
                }
                Key::Down => {
                    app.page_headers.next();
                }
                Key::Up => {
                    app.page_headers.previous();
                }
                _ => {}
            },
            Event::Tick => {
                // app.advance();
            }
        }
    }

    Ok(())
}
