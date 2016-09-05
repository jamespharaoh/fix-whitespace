extern crate regex;

#[macro_use]
extern crate lazy_static;

use regex::Regex;

use std::borrow::Cow;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::iter;

struct Config {
	expand_tabs: bool,
	tab_size: usize,
	line_length: usize,
}

const DEFAULT_CONFIG: Config =
	Config {
		expand_tabs: false,
		tab_size: 4,
		line_length: 80,
	};

fn find_modeline (
	input: & mut Read,
) -> Result <Option <String>, String> {

	let modeline_regex =
		match Regex::new (
			r" (vim|vi|ex): (.+)") {

		Ok (regex) =>
			regex,

		Err (error) =>
			return Err (
				format! (
					"Regex error: {}",
					error)),

	};

	let buf_reader =
		BufReader::new (
			input);

	let mut modeline: Option <String> =
		None;

	for line_result in buf_reader.lines () {

		let line =
			match line_result {

			Ok (line) =>
				line,

			Err (error) =>
				return Err (
					error.description ().to_owned ()),

		};

		match modeline_regex.captures (
			& line) {

			Some (captures) =>
				modeline =
					Some (
						captures.at (2).unwrap ().to_owned ()),

			None =>
				(),

		};

	}

	Ok (modeline)

}

fn config_from_modeline (
	modeline: & str,
) -> Config {

	let mut config =
		DEFAULT_CONFIG;

	for modeline_part
	in modeline.split (' ') {

		if modeline_part == "et" {
			config.expand_tabs = true;
		}

		if modeline_part == "noet" {
			config.expand_tabs = false;
		}

		if modeline_part.starts_with ("ts=") {

			match modeline [3 .. ].parse::<usize> () {

				Ok (tab_size) => {

					config.tab_size =
						tab_size;

				},

				Err (_) =>
					(),

			};

		}

	}

	config

}

fn check_line (
	config: & Config,
	line: & str,
) -> bool {

	if config.expand_tabs && line.contains ('\t') {
		return false;
	}

	if

		! config.expand_tabs

		&& line.chars ().skip_while (
			|character|
			* character == '\t'
		).any (
			|character|
			character == '\t'
		)

	{
		return false;
	}

	if line.ends_with ('\r') {
		return false;
	}

	if line.ends_with ("\r\n") {
		return false;
	}

	if line.len () > 1 {

		let mut line_chars =
			line.chars ();

		line_chars.next_back ();

		let last_character =
			line_chars.next_back ().unwrap ();

		if last_character.is_whitespace () {
			return false;
		}

	}

	let line_len =
		line.len ()
			+ line.matches ('\t').count () * (config.tab_size - 1)
			- 1;

	if line_len > config.line_length {
		return false;
	}

	true

}

fn check_file (
	config: & Config,
	input: & mut Read,
) -> Result <bool, String> {

	let mut buf_reader =
		BufReader::new (
			input);

	let mut line =
		String::new ();

	loop {

		line.truncate (0);

		match buf_reader.read_line (
			& mut line) {

			Ok (0) =>
				return Ok (true),

			Ok (_) => {

				if ! check_line (
					config,
					& line) {
					
					return Ok (false);

				}

			},

			Err (error) => {

				return Err (
					error.description ().to_owned ());

			},

		};

	}

}

fn fix_line <'a> (
	config: & Config,
	filename: & str,
	line_number: u64,
	line: & 'a str,
) -> Cow <'a, str> {

	if check_line (
		config,
		& line) {

		return Cow::Borrowed (
			line);

	}

	let mut modified_line =
		Cow::Borrowed (
			line);

	let mut fixes_applied: Vec <& 'static str> =
		vec! [];

	// fix line ending

	if modified_line.ends_with ('\r') {

		modified_line = {

			let mut line_chars =
				modified_line.chars ();

			line_chars.next_back ();

			Cow::Owned (
				line_chars
					.chain (
						Some (
							'\n'))
					.collect::<String> ()
			)

		};

		fixes_applied.push (
			"fixed mac line ending");

	}

	if modified_line.ends_with ("\r\n") {

		modified_line = {

			let mut line_chars =
				modified_line.chars ();

			line_chars.next_back ();
			line_chars.next_back ();

			Cow::Owned (
				line_chars
					.chain (
						Some (
							'\n'))
					.collect::<String> ()
			)

		};

		fixes_applied.push (
			"fixed windows line ending");

	}

	// expand tabs

	if config.expand_tabs && modified_line.contains ('\t') {

		let tab_as_spaces =
			iter::repeat (" ")
				.take (config.tab_size)
				.collect::<String> ();

		modified_line =
			Cow::Owned (
				modified_line.replace (
					"\t",
					& tab_as_spaces));

		fixes_applied.push (
			"expanded tabs");

	}

	// detect tabs after other characters

	if

		! config.expand_tabs

		&& modified_line.chars ().skip_while (
			|character|
			* character == '\t'
		).any (
			|character|
			character == '\t'
		)

	{

		fixes_applied.push (
			"tabs after other characters");

	}

	// fix whitespace at end

	if modified_line.len () > 1 {

		let last_character = {

			let mut line_chars =
				modified_line.chars ();

			line_chars.next_back ();

			line_chars.next_back ().unwrap ()

		};

		if last_character.is_whitespace () {

			fixes_applied.push (
				"removed whitespace from end");

			modified_line = {

				let mut line_chars =
					modified_line.chars ();

				line_chars.next_back ();

				let modified_line_temp;

				loop {

					match line_chars.next_back () {

						Some (next_last_character) =>
							if ! next_last_character.is_whitespace () {

							modified_line_temp =
								line_chars
									.chain (
										Some (
											next_last_character))
									.chain (
										Some (
											'\n'))
									.collect::<String> ();

							break;

						},

						None => {

							modified_line_temp =
								"\n".to_string ();

							break;

						},

					}

				}

				Cow::Owned (
					modified_line_temp)

			};

		}

	}

	// detect long lines

	let modified_line_len =
		modified_line.len ()
			+ modified_line.matches ('\t').count () * (config.tab_size - 1)
			- 1;

	if modified_line_len > config.line_length {

		fixes_applied.push (
			"line too long");
	
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
	input: & mut Read,
	output: & mut Write,
) -> Result <(), String> {

	let mut buf_reader =
		BufReader::new (
			input);

	let mut line =
		String::new ();

	let mut line_number: u64 =
		0;

	loop {

		line.truncate (0);

		match buf_reader.read_line (
			& mut line) {

			Ok (0) =>
				return Ok (()),

			Ok (_) => {

				let output_line =
					fix_line (
						config,
						filename,
						line_number,
						& line);

				match output.write_all (
					& output_line.as_bytes ()) {

					Ok (_) =>
						(),

					Err (error) => {

						return Err (
							error.description ().to_owned ());

					},

				};

			},

			Err (error) => {

				return Err (
					error.description ().to_owned ());

			},

		};

		line_number += 1;

	};

}

fn do_file (
	filename: & str,
) {

	// open file

	let mut file =
		match File::open (
			filename) {

		Ok (file) =>
			file,

		Err (error) => {

			println! (
				"Error opening {}: {}",
				filename,
				error.description ());

			return;

		},

	};

	// first pass - look for modeline

	let modeline =
		match find_modeline (
			& mut file) {

		Ok (modeline) =>
			modeline,

		Err (error) => {

			println! (
				"Error reading {}: {}",
				filename,
				error);

			return;

		},

	};

	let config =
		match modeline {

		Some (modeline) =>
			config_from_modeline (
				& modeline),

		None =>
			DEFAULT_CONFIG,

	};

	// second pass - look for problems

	match file.seek (
		SeekFrom::Start (0)) {

		Ok (_) =>
			(),

		Err (error) => {

			println! (
				"Error reading {}: {}",
				filename,
				error.description ());

			return;

		}

	}

	match check_file (
		& config,
		& mut file) {

		Ok (true) =>
			return,

		Ok (false) =>
			(),

		Err (error) => {

			println! (
				"Error reading {}: {}",
				filename,
				error);

			return;

		},

	};

	// third pass - correct or report problems

	match file.seek (
		SeekFrom::Start (0)) {

		Ok (_) =>
			(),

		Err (error) => {

			println! (
				"Error reading {}: {}",
				filename,
				error.description ());

			return;

		}

	}

	let output_filename =
		format! (
			"{}.tmp",
			filename);

	let mut output =
		match File::create (
			& output_filename) {

		Ok (file) =>
			file,

		Err (error) => {

			println! (
				"Error creating {}: {}",
				output_filename,
				error.description ());

			return;

		},

	};

	match fix_file (
		& config,
		filename,
		& mut file,
		& mut output) {

		Ok (_) =>
			(),

		Err (error) => {

			println! (
				"Error fixing {}: {}",
				filename,
				error);

			return;

		}

	};

    let metadata =
    	match std::fs::metadata (
    		filename) {

		Ok (metadata) =>
			metadata,

		Err (error) => {

			println! (
				"Error reading permissions for {}: {}",
				filename,
				error.description ());

			return;

		},

    };

	match fs::set_permissions (
		& output_filename,
		metadata.permissions ()) {

		Ok (_) =>
			(),

		Err (error) => {

			println! (
				"Error setting permissions for {}: {}",
				output_filename,
				error.description ());

			return;

		}

	};

	match fs::rename (
		& output_filename,
		filename) {

		Ok (_) =>
			(),

		Err (error) => {

			println! (
				"Error renaming {} to {}: {}",
				output_filename,
				filename,
				error.description ());

		}

	}

}

fn main () {

	for filename in env::args ().skip (1) {

		do_file (
			& filename);

	}

}

// ex: noet ts=4 filetype=rust
