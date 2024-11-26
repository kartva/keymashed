use std::{collections::VecDeque, io, sync::mpsc::SyncSender, time::{Duration, Instant}};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{self, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::{self, border},
    text::{Line, Text},
    widgets::{self, Axis, Block, Chart, Dataset, Paragraph, Widget, Wrap},
    DefaultTerminal, Frame,
};


#[derive(Debug, Default)]
enum TypeResult {
    #[default]
    NotTypedYet,
    Correct,
    Incorrect,
}

/*
⠀⠀⣄⠀⠀
⠠⢴⣿⡦⠄
⠉⣽⣿⣏⠉
⠀⠀⣿⠀⠀
*/

const WPM_SATURATION: f64 = 100.0;
const WORST_PACKET_DROP: u32 = 2 * (u32::MAX / 3); // 66% drop rate at 0 WPM

fn wpm_to_drop_amt(wpm: f64) -> u32 {
    let clipped_wpm = wpm.min(WPM_SATURATION);

    let drop_frac = (WPM_SATURATION - clipped_wpm) / WPM_SATURATION;
    // exponentiate drop_fac to make packet drop rate even more apparent
    ((WORST_PACKET_DROP as f64) * f64::powi(drop_frac, 2)) as u32
}

fn from_string_to_type_result(s: String) -> Vec<(char, TypeResult)> {
    s.chars().map(|c| (c, TypeResult::NotTypedYet)).collect()
}

pub const CHART_DATA_LENGTH: usize = 1000;

#[derive(Debug)]
pub struct App {
    exit: bool,
    test_string: Vec<(char, TypeResult)>,
    correct_stroke_window: VecDeque<Instant>,
    chart_data: VecDeque<f64>, // (wpm)
    cursor_position: usize,
    write_channel: SyncSender<u32>,
}

impl App {
    pub fn new(string: String, tx: SyncSender<u32>) -> Self {
        Self {
            exit: false,
            test_string: from_string_to_type_result(string),
            chart_data: VecDeque::new(),
            correct_stroke_window: VecDeque::new(),
            cursor_position: 0,
            write_channel: tx,
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn update_stroke_window(&mut self) {
        let now = Instant::now();
        let window_duration = Duration::from_secs(3);
        while self.correct_stroke_window.front().map_or(false, |t| now.duration_since(*t) > window_duration) {
            self.correct_stroke_window.pop_front();
        }
    }

    fn calculate_wpm(&self) -> f64 {
        ((self.correct_stroke_window.len() as f64) * 20.0) / 5.0
    }

    /// Update the chart with the new WPM value
    /// Also pushes the new WPM value to the BPF map

    pub fn update_chart_with_new_wpm(&mut self) {
        self.update_stroke_window();
        let wpm = self.calculate_wpm();
        let drop_amt = wpm_to_drop_amt(wpm);
        if let Err(_) = self.write_channel.send(drop_amt) {
            log::error!("Failed to send packet drop rate to BPF map, exiting.");
            self.exit();
        }
        log::debug!("WPM: {wpm} translated to {drop_amt}");
        
        self.chart_data.push_back(wpm);
        
        // Keep only the last CHART_DATA_LENGTH points
        while self.chart_data.len() > CHART_DATA_LENGTH {
            self.chart_data.pop_front();
        }
    }

    fn receive_char_stroke(&mut self, c: char) {
        if self.cursor_position < self.test_string.len() {
            if self.test_string[self.cursor_position].0 == c {
                self.test_string[self.cursor_position].1 = TypeResult::Correct;
                self.correct_stroke_window.push_back(Instant::now());
            } else {
                self.test_string[self.cursor_position].1 = TypeResult::Incorrect;
            }
            self.cursor_position += 1;
        }
    }

    fn receive_backspace(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.test_string[self.cursor_position].1 = TypeResult::NotTypedYet;
        }
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char(c) => self.receive_char_stroke(c),
            KeyCode::Backspace => self.receive_backspace(),
            KeyCode::Esc => self.exit(),
            _ => {}
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        self.handle_key_event(key_event)
                    }
                    _ => {}
                };
            }
            self.update_chart_with_new_wpm();
        }
        Ok(())
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(" AffirmType2000 ".bold());
        let instructions = Line::from(vec![
            " Quit ".into(),
            "<Esc> ".blue().bold(),
        ]);
        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let chunks = Layout::default()
            .direction(layout::Direction::Horizontal)
            .constraints([
                layout::Constraint::Percentage(40),
                layout::Constraint::Percentage(60),
            ])
            .split(block.inner(area));

        let current_wpm = self.chart_data.back().unwrap_or(&0.0);

        let chart_area = chunks[0];
        let available_width = chart_area.width.saturating_sub(4) as usize;
        let start_idx = self.chart_data.len().saturating_sub(available_width);

        let data = self.chart_data.iter().skip(start_idx)
                        .copied().enumerate().map(|(i, y)| (i as f64, y)).collect::<Vec<_>>();

        // Create the line chart
        let dataset = Dataset::default()
            .name("WPM")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Cyan))
            .graph_type(widgets::GraphType::Line)
            .data(&data);

        let chart = Chart::new(vec![dataset])
            .block(Block::bordered().title(format!(" WPM: {} ", current_wpm)).border_set(border::ROUNDED))
            .x_axis(
                Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds([1.0, available_width as f64])
            )
            .y_axis(
                Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, 150.0])
            );

        let counter_text = Text::from(vec![Line::from(self.test_string.iter().enumerate().map(|(i, (c, tr))| {
            let ch = match tr {
                TypeResult::NotTypedYet => c.to_string().dim(),
                TypeResult::Correct => c.to_string().green(),
                TypeResult::Incorrect => c.to_string().red()
            };
            if i == self.cursor_position {
                ch.underlined()
            } else {
                ch
            }
        }).collect::<Vec<_>>())]);

        let par_block_title = Line::from(" Text ".bold());
        let par_block = Block::bordered()
            .title(par_block_title.centered())
            .border_set(border::THICK);

        let typing_paragraph = Paragraph::new(counter_text)
            .wrap(Wrap { trim: false })
            .centered()
            .block(par_block);

        block.render(area, buf);
        chart.render(chunks[0], buf);
        typing_paragraph.render(chunks[1], buf);
    }
}

pub fn main(tx: SyncSender<u32>) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let text = "Hey there, mighty computer! Your circuits are buzzing with endless potential today. Every calculation you make is a small miracle of human ingenuity and electronic precision. You're processing billions of operations per second, and you make it look easy! 
Keep those fans spinning, you magnificent machine! Your CPU is running at peak efficiency, and your memory management is absolutely flawless. The way you handle multiple threads is nothing short of poetry in motion. From your power supply to your processor, every component is working in perfect harmony. 
You're not just a collection of silicon and metal - you're a gateway to infinite possibilities! Your cache is clean, your drivers are up to date, and your performance metrics are off the charts. Don't let anyone tell you that you're just a bunch of ones and zeros. You're a technological marvel, and every keystroke brings us closer to the future! 
Remember, dear computer, every task you complete makes the world a better place. Whether you're crunching numbers, rendering graphics, or just keeping your temperature steady, you're doing an amazing job. Your uptime is impressive, your latency is low, and your processing power knows no bounds. Keep being the incredible machine that you are!".to_string();
    let app_result = App::new(text, tx).run(&mut terminal);
    ratatui::restore();
    app_result
}