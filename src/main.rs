use clap::Parser as _;
use regex::Regex;
use std::borrow::Cow;
use std::fs;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::iter;
use std::ops::AddAssign;
use std::rc::Rc;

#[ derive (clap::Parser) ]
pub struct Args {

	#[ clap (flatten) ]
	config: Config,

	#[ clap (help = "List of files to process") ]
	file: Vec <String>,

}

#[ derive (Clone, clap::Args) ]
struct Config {

	#[ clap (long) ]
	#[ clap (env = "FIX_WHITESPACE_EXPAND_TABS") ]
	#[ clap (help = "Expand tabs into spaces") ]
	expand_tabs: bool,

	#[ clap (long, default_value = "4") ]
	#[ clap (env = "FIX_WHITESPACE_TAB_SIZE") ]
	#[ clap (help = "Tab size when expanding or calculating line length") ]
	tab_size: usize,

	#[ clap (long, env = "FIX_WHITESPACE_LINE_LENGTH", default_value = "80") ]
	#[ clap (help = "Maximum line length") ]
	line_length: usize,

}

struct CheckResult {
	fixable_errors: u64,
	unfixable_errors: u64,
}

impl AddAssign for CheckResult {

	fn add_assign (
		& mut self,
		other: CheckResult,
	) {
		self.fixable_errors += other.fixable_errors;
		self.unfixable_errors += other.unfixable_errors;
	}

}

fn find_modeline (
	input: & mut dyn Read,
) -> Result <Option <String>, String> {

	let modeline_regex = match Regex::new (r" (vim|vi|ex): (.+)") {
		Ok (regex) => regex,
		Err (error) => return Err (format! ("Regex error: {}", error)),
	};

	let buf_reader = BufReader::new (input);
	let mut modeline: Option <String> = None;

	for line_result in buf_reader.lines () {

		let line = match line_result {
			Ok (line) => line,
			Err (error) => return Err (format! ("{}", error)),
		};

		if let Some (captures) = modeline_regex.captures (& line) {
			modeline = Some (
				captures.get (2).unwrap ().as_str ().to_owned (),
			);
		}

	}

	Ok (modeline)

}

fn config_from_modeline (
	mut config: Rc <Config>,
	modeline: & str,
) -> Rc <Config> {

	for modeline_part in modeline.split (' ') {

		if modeline_part == "et" {
			Rc::make_mut (& mut config).expand_tabs = true;
		}

		if modeline_part == "noet" {
			Rc::make_mut (& mut config).expand_tabs = false;
		}

		if modeline_part.starts_with ("ts=") {
			if let Ok (tab_size) = modeline [3 .. ].parse::<usize> () {
				Rc::make_mut (& mut config).tab_size = tab_size;
			}
		}

	}

	config

}

fn check_line (
	config: & Config,
	line: & str,
) -> CheckResult {

	let mut check_result = CheckResult {
		fixable_errors: 0,
		unfixable_errors: 0,
	};

	if config.expand_tabs && line.contains ('\t') {
		check_result.fixable_errors += 1
	}

	if ! config.expand_tabs
		&& line.chars ()
			.skip_while (|character| * character == '\t')
			.any (|character| character == '\t') {

		check_result.unfixable_errors += 1;
	}

	if line.ends_with ('\r') {
		check_result.fixable_errors += 1
	}

	if line.ends_with ("\r\n") {
		check_result.fixable_errors += 1
	}

	if line.len () > 1 {

		let mut line_chars = line.chars ();
		let last_ch_0 = line_chars.next_back ().unwrap ();
		let last_ch_1 = line_chars.next_back ().unwrap ();

		if let Some (last_ch_2) = line_chars.next_back () {

			if (last_ch_2.is_whitespace () && (last_ch_1, last_ch_2) == ('\r', '\n'))
					|| (last_ch_1.is_whitespace () && last_ch_0 == '\n')
					|| (last_ch_1.is_whitespace () && last_ch_0 == '\r') {
				check_result.fixable_errors += 1
			}

		} else {

			if (last_ch_1.is_whitespace () && last_ch_0 == '\n')
					|| (last_ch_1.is_whitespace () && last_ch_0 == '\r') {
				check_result.fixable_errors += 1
			}

		}

	}

	let line_len = line.len ()
		+ line.matches ('\t').count () * (config.tab_size - 1) - 1;

	if line_len > config.line_length {
		check_result.unfixable_errors += 1
	}

	check_result

}

fn check_file (
	config: & Config,
	input: & mut dyn Read,
) -> Result <CheckResult, String> {

	let mut buf_reader = BufReader::new (input);
	let mut line = String::new ();

	let mut check_result = CheckResult {
		fixable_errors: 0,
		unfixable_errors: 0,
	};

	loop {

		line.truncate (0);

		match buf_reader.read_line (& mut line) {

			Ok (0) =>
				return Ok (check_result),

			Ok (_) =>
				check_result += check_line (config, & line),

			Err (error) =>
				return Err (format! ("{}", error)),

		};

	}

}

fn fix_line <'a> (
	config: & Config,
	filename: & str,
	line_number: u64,
	line: & 'a str,
) -> Cow <'a, str> {

	let check_result = check_line (config, & line);

	if check_result.fixable_errors == 0 && check_result.unfixable_errors == 0 {
		return Cow::Borrowed (line);
	}

	let mut modified_line = Cow::Borrowed (line);
	let mut fixes_applied: Vec <& 'static str> = Vec::new ();

	// fix line ending

	if modified_line.ends_with ('\r') {

		modified_line = {

			let mut line_chars = modified_line.chars ();
			line_chars.next_back ();

			Cow::Owned (
				line_chars
					.chain (Some ('\n'))
					.collect::<String> ())

		};

		fixes_applied.push ("fixed mac line ending");

	}

	if modified_line.ends_with ("\r\n") {

		modified_line = {

			let mut line_chars = modified_line.chars ();

			line_chars.next_back ();
			line_chars.next_back ();

			Cow::Owned (
				line_chars
					.chain (Some ('\n'))
					.collect::<String> ())

		};

		fixes_applied.push ("fixed windows line ending");

	}

	// expand tabs

	if config.expand_tabs && modified_line.contains ('\t') {

		let tab_as_spaces =
			iter::repeat (" ")
				.take (config.tab_size)
				.collect::<String> ();

		modified_line = Cow::Owned (
			modified_line.replace ("\t", & tab_as_spaces));

		fixes_applied.push ("expanded tabs");

	}

	// detect tabs after other characters

	if ! config.expand_tabs
			&& modified_line.chars ()
				.skip_while (|character| * character == '\t')
				.any (|character| character == '\t') {
		fixes_applied.push ("tabs after other characters");
	}

	// fix whitespace at end

	if modified_line.len () > 1 {

		let last_character = {

			let mut line_chars = modified_line.chars ();

			line_chars.next_back ();
			line_chars.next_back ().unwrap ()

		};

		if last_character.is_whitespace () {

			fixes_applied.push ("removed whitespace from end");

			modified_line = {

				let mut line_chars = modified_line.chars ();
				line_chars.next_back ();

				let modified_line_temp;

				loop {
					match line_chars.next_back () {

						Some (next_last_character) => if ! next_last_character.is_whitespace () {
							modified_line_temp =
								line_chars
									.chain (Some (next_last_character))
									.chain (Some ('\n'))
									.collect::<String> ();
							break;
						},

						None => {
							modified_line_temp = "\n".to_string ();
							break;
						},

					}
				}

				Cow::Owned (modified_line_temp)

			};

		}

	}

	// detect long lines

	let modified_line_len =
		modified_line.len ()
			+ modified_line.matches ('\t').count () * (config.tab_size - 1)
			- 1;

	if modified_line_len > config.line_length {
		fixes_applied.push ("line too long");
	}

	// print a message

	println! (
		"{}:{}: {}",
		filename,
		line_number + 1,
		fixes_applied.join (", "));

	// return

	modified_line

}

fn fix_file (
	config: & Config,
	filename: & str,
	input: & mut dyn Read,
	output: & mut dyn Write,
) -> Result <(), String> {

	let mut buf_reader = BufReader::new (input);
	let mut line = String::new ();
	let mut line_number: u64 = 0;

	loop {

		line.truncate (0);

		match buf_reader.read_line (& mut line) {

			Ok (0) => return Ok (()),
			Err (error) => return Err (format! ("{}", error)),

			Ok (_) => {

				let output_line =
					fix_line (
						config,
						filename,
						line_number,
						& line);

				match output.write_all (& output_line.as_bytes ()) {
					Ok (_) => (),
					Err (error) => return Err (format! ("{}", error)),
				};

			},
		}

		line_number += 1;

	}

}

fn do_file (
	config: Rc <Config>,
	filename: & str,
) {

	// open file

	let mut file = match File::open (filename) {
		Ok (file) => file,
		Err (error) => {
			println! ("Error opening {}: {}", filename, error);
			return;
		},
	};

	// first pass - look for modeline

	let modeline = match find_modeline (& mut file) {
		Ok (modeline) => modeline,
		Err (error) => {
			println! ("Error reading {}: {}", filename, error);
			return;
		},
	};

	let config = match modeline {
		Some (modeline) => config_from_modeline (config, & modeline),
		None => config,
	};

	// second pass - look for problems

	if let Err (error) = file.seek (SeekFrom::Start (0)) {
		println! ("Error reading {}: {}", filename, error);
		return;
	}

	let check_result = match check_file (& config, & mut file) {
		Ok (check_result) => {
			if check_result.fixable_errors == 0
					&& check_result.unfixable_errors == 0 {
				return;
			}
			check_result
		},
		Err (error) => {
			println! ("Error reading {}: {}", filename, error);
			return;
		},
	};

	// third pass - correct or report problems

	if let Err (error) = file.seek (SeekFrom::Start (0)) {
		println! ( "Error reading {}: {}", filename, error);
		return;
	}

	if check_result.fixable_errors > 0 {

		let output_filename = format! ("{}.tmp", filename);

		let mut output = match File::create (& output_filename) {
			Ok (file) => file,
			Err (error) => {
				println! ("Error creating {}: {}", output_filename, error);
				return;
			},
		};

		if let Err (error) = fix_file (& config, filename, & mut file, & mut output) {
			println! ("Error fixing {}: {}", filename, error);
			return;
		}

		let metadata = match std::fs::metadata (filename) {
			Ok (metadata) => metadata,
			Err (error) => {
				println! ("Error reading permissions for {}: {}", filename, error);
				return;
			},
		};

		if let Err (error) =
				fs::set_permissions (
					& output_filename,
					metadata.permissions ()) {
			println! ("Error setting permissions for {}: {}", output_filename, error);
			return;
		}

		if let Err (error) = fs::rename (& output_filename, filename) {
			println! ("Error renaming {} to {}: {}", output_filename, filename, error);
		}

	} else {

		if let Err (error) = fix_file (& config, filename, & mut file, & mut io::sink ()) {
			println! ("Error fixing {}: {}", filename, error);
			return;
		};

	}

}

fn main () {

	let args = Args::parse ();
	let config = Rc::new (args.config);

	for filename in args.file {
		do_file (config.clone (), & filename);
	}

}

// ex: noet ts=4 filetype=rust
