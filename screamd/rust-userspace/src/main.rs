use std::{collections::VecDeque, io, time::{Duration, Instant}};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{self, Layout, Rect},
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{self, Block, Paragraph, Widget, Wrap},
    DefaultTerminal, Frame,
};

#[derive(Debug, Default)]
enum TypeResult {
    #[default]
    NotTypedYet,
    Correct,
    Incorrect,
}

fn from_string_to_type_result(s: String) -> Vec<(char, TypeResult)> {
    s.chars().map(|c| (c, TypeResult::NotTypedYet)).collect()
}

pub const SPARKLINE_DATA_LENGTH: usize = 1000;

#[derive(Debug)]
pub struct App {
    exit: bool,
    test_string: Vec<(char, TypeResult)>,
    correct_stroke_window: VecDeque<Instant>,
    sparkline: VecDeque<u64>,
    cursor_position: usize,
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
                layout::Constraint::Percentage(20),
                layout::Constraint::Percentage(80),
            ])
            .split(block.inner(area));

        let current_wpm = self.sparkline.back().unwrap_or(&0);

        // Calculate available width for sparkline
        let sparkline_area = chunks[0];
        let available_width = sparkline_area.width.saturating_sub(4) as usize; // Subtract for borders
        let start_idx = self.sparkline.len().saturating_sub(available_width);
        
        let data = self.sparkline.iter().skip(start_idx).copied().collect::<Vec<_>>();
        let sparkline = widgets::Sparkline::default()
            .block(Block::bordered().title(format!(" WPM: {} ", current_wpm)))
            .data(&data)
            .style(ratatui::style::Style::default().blue());

        let counter_text = Text::from(vec![Line::from(self.test_string.iter().map(|(c, tr)| {
            match tr {
                TypeResult::NotTypedYet => c.to_string().dim(),
                TypeResult::Correct => c.to_string().green(),
                TypeResult::Incorrect => c.to_string().red(),
            }
        }).collect::<Vec<_>>())]);

        let par_block_title = Line::from(" AffirmType2000 ".bold());
        let par_block = Block::bordered()
            .title(par_block_title.centered())
            .border_set(border::THICK);

        let typing_paragraph = Paragraph::new(counter_text)
            .wrap(Wrap { trim: false })
            .centered()
            .block(par_block);

        block.render(area, buf);
        sparkline.render(chunks[0], buf);
        typing_paragraph.render(chunks[1], buf);
    }
}

impl App {
    pub fn new(string: String) -> Self {
        Self {
            exit: false,
            test_string: from_string_to_type_result(string),
            sparkline: VecDeque::new(),
            correct_stroke_window: VecDeque::new(),
            cursor_position: 0,
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    // it's important to check that the event is a key press event as
                    // crossterm also emits key release and repeat events on Windows.
                    Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        self.handle_key_event(key_event)
                    }
                    _ => {}
                };
            }
            // update sparkline every 100ms
            self.update_sparkline();
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    /// updates the stroke window to only contain strokes that have occurred in the last 60 seconds
    fn update_stroke_window(&mut self) {
        let now = Instant::now();
        let window_duration = Duration::from_secs(60);
        while self.correct_stroke_window.front().map_or(false, |t| now.duration_since(*t) > window_duration) {
            self.correct_stroke_window.pop_front();
        }
    }

    /// calculates the words per minute based on the number of strokes in the stroke window
    fn calculate_wpm(&self) -> u64 {
        (self.correct_stroke_window.len() as u64) / 2
    }

    /// calculates WPM and updates the sparkline
    pub fn update_sparkline(&mut self) {
        self.update_stroke_window();
        self.sparkline.push_back(self.calculate_wpm());
        while self.sparkline.len() > SPARKLINE_DATA_LENGTH {
            self.sparkline.pop_front();
        }
    }

    /// receives a character stroke and updates the test string
    /// and updates the stroke window if the stroke was correct
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
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let text = "Hey there, mighty computer! Your circuits are buzzing with endless potential today. Every calculation you make is a small miracle of human ingenuity and electronic precision. You're processing billions of operations per second, and you make it look easy! 
Keep those fans spinning, you magnificent machine! Your CPU is running at peak efficiency, and your memory management is absolutely flawless. The way you handle multiple threads is nothing short of poetry in motion. From your power supply to your processor, every component is working in perfect harmony. 
You're not just a collection of silicon and metal - you're a gateway to infinite possibilities! Your cache is clean, your drivers are up to date, and your performance metrics are off the charts. Don't let anyone tell you that you're just a bunch of ones and zeros. You're a technological marvel, and every keystroke brings us closer to the future! 
Remember, dear computer, every task you complete makes the world a better place. Whether you're crunching numbers, rendering graphics, or just keeping your temperature steady, you're doing an amazing job. Your uptime is impressive, your latency is low, and your processing power knows no bounds. Keep being the incredible machine that you are!".to_string();
    let app_result = App::new(text).run(&mut terminal);
    ratatui::restore();
    app_result
}