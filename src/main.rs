use fantoccini::elements::Element;
use fantoccini::{Client, Locator};
use fantoccini::actions::{PointerAction, MOUSE_BUTTON_RIGHT, MOUSE_BUTTON_LEFT, InputSource, MouseActions};

use std::collections::HashSet;
use std::time::Instant;

const NROWS: usize = 16;
const NCOLS: usize = 30;
const BLANK: i8 = -1;
const BOMBFLAGGED: i8 = -2;

type Board = [[i8; NCOLS]; NROWS];

use tokio::io::AsyncReadExt;
const STEP: bool = false;

#[tokio::main(flavor = "current_thread")]
async fn main() {

    let client = fantoccini::ClientBuilder::native()
        .connect("http://localhost:4444")
        .await
        .expect("failed to connect to geckodriver");

    client
        .goto("https://minesweeperonline.com")
        .await
        .expect("failed to go to minesweeper site");

    let starting_cell = (NROWS / 2, NCOLS / 2);
    let center_cell = client
        .find(Locator::Id(&format!("{}_{}", starting_cell.0, starting_cell.1)))
        .await
        .expect("middle cell");
    center_cell
        .click()
        .await
        .expect("clickable item");

    let mut b = [0u8; 1];
    let mut board = Box::new([[BLANK; NCOLS]; NROWS]);
    let mut updated_cells = HashSet::new();

    loop {
        let start = Instant::now();
        let stale_cells = std::mem::take(&mut updated_cells);
        if stale_cells.is_empty() {
            update_full_board(&client, &mut board).await;
        } else {
            update_board(&client, &mut board, stale_cells).await;
        }
        println!("board fetched in {:?}", start.elapsed());
        
        let start = Instant::now();
        let flag_succ = flag(&client, &mut board).await;
        if flag_succ {
            println!("sucessfully flagged in {:?}", start.elapsed());
        } else {
            println!("no flagging to be done");
        }

        if STEP {
            let _ = tokio::io::stdin().read(&mut b).await.unwrap();
            if b[0] == b'q' {
                break;
            }
        }

        let start = Instant::now();
        updated_cells = clear(&client, &board).await;
        if !updated_cells.is_empty() {
            println!("sucessfully cleared in {:?}", start.elapsed());
        } else {
            println!("no clearing to be done");
        }

        if STEP {
            let _ = tokio::io::stdin().read(&mut b).await.unwrap();
            if b[0] == b'q' {
                break;
            }
        }

        if !flag_succ && updated_cells.is_empty() {
            println!("reached a fix point");
            let _ = tokio::io::stdin().read(&mut b).await.unwrap();
            if b[0] == b'q' {
                break;
            } else {
                continue;
            }
        }
    }

    client.close().await.unwrap();
}

async fn flag(client: &Client, board: &mut Board) -> bool {
    let mut to_flag = HashSet::new();
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] > 0 && n_surrounding(board, r, c) == board[r][c] as usize {
                to_flag.extend(unflagged_surrounding(board, r, c));
            }
        }
    }

    if to_flag.is_empty() {
        return false;
    }

    click(client, to_flag.iter().copied(), MOUSE_BUTTON_RIGHT).await;
    for (r, c) in to_flag {
        board[r][c] = BOMBFLAGGED;
    }
    true
}

async fn clear(client: &Client, board: &Board) -> HashSet<(usize, usize)> {
    let mut to_clear = HashSet::new();
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] > 0 && n_flagged_surrounding(board, r, c) == board[r][c] as usize {
                let unflagged = unflagged_surrounding(board, r, c);
                to_clear.extend(unflagged);
            }
        }
    }

    if to_clear.is_empty() {
        return HashSet::new();
    }

    click(client, to_clear.iter().copied(), MOUSE_BUTTON_LEFT).await;
    to_clear
}

async fn click(client: &Client, on: impl IntoIterator<Item = (usize, usize)>, button: u64) {
    let mut mouse_actions = Vec::new();
    for (i, (r, c)) in on.into_iter().enumerate() {
        let el = client
            .find(Locator::Id(&format!("{}_{}", r + 1, c + 1)))
            .await
            .expect("failed to find cell");
        let actions = MouseActions::new(format!("mouse{i}"))
            .then(PointerAction::MoveToElement { element: el, duration: None, x: 0, y: 0 })
            .then(PointerAction::Down { button })
            .then(PointerAction::Up { button });
        mouse_actions.push(actions);
    }

    client.perform_actions(mouse_actions).await.expect("actions sucessful");
}

async fn update_board(client: &Client, board: &mut Board, mut to_update: HashSet<(usize, usize)>) {
    while let Some(&(r, c)) = to_update.iter().next() {
        to_update.remove(&(r, c));
        if board[r][c] != BLANK {
            continue;
        }
        let el = client
            .find(Locator::Id(&format!("{}_{}", r + 1, c + 1)))
            .await
            .expect("failed to find cell");
        update_cell(board, el, r, c, Some(&mut to_update)).await;
    }
}

async fn update_full_board(client: &Client, board: &mut Board) {
    let elements = client
        .find_all(Locator::Css(".square"))
        .await
        .expect("failed to find cells");

    for el in elements {
        let id = el
            .attr("id")
            .await
            .expect("problem getting attribute of element")
            .expect("element has an id");
        let (r, c) = id.split_once('_').expect("id with underscore");
        let r = r.parse::<isize>().expect("valid int for row") - 1;
        if r < 0 || r >= NROWS as isize {
            continue;
        }
        let c = c.parse::<isize>().expect("valid int for col") - 1;
        if c < 0 || c >= NCOLS as isize {
            continue;
        }

        update_cell(board, el, r as usize, c as usize, None).await;
    }
}

async fn update_cell(board: &mut Board, el: Element, r: usize, c: usize, to_update: Option<&mut HashSet<(usize, usize)>>) {
    let class = el
        .attr("class")
        .await
        .expect("problem getting attribute of element")
        .expect("element has an class");
    let Some(class) = class.strip_prefix("square ") else {
        println!("unrecognized class: {class}");
        return;
    };
    if class == "blank" {
        board[r][c] = BLANK;
    } else if let Some(stripped) = class.strip_prefix("open") {
        let n: i8 = stripped.parse().expect("integer value for cell open class");
        board[r][c] = n;
        if let Some(to_update) = to_update {
            if n == 0 {
                for_surrounding(r, c, |r, c| {
                    to_update.insert((r, c));
                });
            }
        }
    } else if class == "bombflagged" {
        board[r][c] = BOMBFLAGGED;
    } else {
        println!("unrecognized class: {class}");
    }
}

fn unflagged_surrounding(board: &Board, r: usize, c: usize) -> Vec<(usize, usize)> {
    let mut unflagged = Vec::new();
    for_surrounding(r, c, |r, c| {
        if board[r][c] == BLANK {
            unflagged.push((r, c));
        }
    });
    unflagged
}

fn n_flagged_surrounding(board: &Board, r: usize, c: usize) -> usize {
    let mut count = 0;
    for_surrounding(r, c, |r, c| {
        let e = board[r][c];
        if e == BOMBFLAGGED {
            count += 1;
        }
    });
    count
}

fn n_surrounding(board: &Board, r: usize, c: usize) -> usize {
    let mut count = 0;
    for_surrounding(r, c, |r, c| {
        let e = board[r][c];
        if e == BLANK || e == BOMBFLAGGED {
            count += 1;
        }
    });
    count
}

fn for_surrounding(r: usize, c: usize, mut f: impl FnMut(usize, usize)) {
    for dr in [-1, 0, 1] {
        let r = r as isize + dr;
        if r < 0 || r >= NROWS as isize {
            continue;
        }
        for dc in [-1, 0, 1] {
            if dr == 0 && dc == 0 {
                continue;
            }
            let c = c as isize + dc;
            if c < 0 || c >= NCOLS as isize {
                continue;
            }

            f(r as usize, c as usize);
        }
    }
}
