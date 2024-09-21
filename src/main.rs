use fantoccini::elements::Element;
use fantoccini::{Client, Locator};
use fantoccini::actions::{PointerAction, MOUSE_BUTTON_RIGHT, MOUSE_BUTTON_LEFT, InputSource, MouseActions};

use std::collections::HashSet;
use std::time::Instant;

const NROWS: usize = 16;
const NCOLS: usize = 30;
const NBOMBS: usize = 99;
const BLANK: i8 = -1;
const BOMBFLAGGED: i8 = -2;

type Board = [[i8; NCOLS]; NROWS];

use tokio::io::AsyncReadExt;
const STEP: bool = true;

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

    let mut b = [0u8; 1];
    let mut board = Box::new([[BLANK; NCOLS]; NROWS]);
    let mut updated_cells = None;

    click_center_cell(&client).await;

    let automated_exit = loop {
        if client.find(Locator::Css(".facewin")).await.is_ok() {
            println!("Victory!");
            break true;
        }
        if let Ok(facedead) = client.find(Locator::Css(".facedead")).await {
            println!("Defeat!");
            if STEP {
                break true;
            } else {
                facedead
                    .click()
                    .await
                    .expect("clickable facedead to reset game");
                click_center_cell(&client).await;
            }
        }

        let start = Instant::now();
        if let Some(stale_cells) = updated_cells {
            update_board(&client, &mut board, stale_cells).await;
        } else {
            update_full_board(&client, &mut board).await;
        }
        println!("board fetched in {:?}", start.elapsed());
        
        let start = Instant::now();
        let flag_succ = if flag(&client, &mut board).await {
            println!("sucessfully flagged in {:?}", start.elapsed());
            true
        } else if flag_harder(&client, &mut board).await {
            println!("sucessfully flagged HARDER in {:?}", start.elapsed());
            true
        } else {
            println!("no flagging to be done");
            false
        };

        if STEP {
            let _ = tokio::io::stdin().read(&mut b).await.unwrap();
            if b[0] == b'q' {
                break false;
            }
        }

        let start = Instant::now();
        // check for non-empty result
        if let Some(cells) = Some(clear(&client, &board).await).filter(|c| !c.is_empty()) {
            println!("sucessfully cleared in {:?}", start.elapsed());
            updated_cells = Some(cells);
        } else if let Some(cells) = Some(clear_harder(&client, &board).await).filter(|c| !c.is_empty()) {
            println!("sucessfully cleared HARDER in {:?}", start.elapsed());
            updated_cells = Some(cells);
        } else {
            println!("no clearing to be done");
            if flag_succ {
                updated_cells = Some(Default::default());
            } else {
                updated_cells = None;
            }
        }

        if STEP {
            let _ = tokio::io::stdin().read(&mut b).await.unwrap();
            if b[0] == b'q' {
                break false;
            }
        }

        if !flag_succ && updated_cells.is_none() {
            if flag_succ {
                if let Some(cells) = check_all_bombs_flagged_then_clear(&client, &mut board).await {
                    updated_cells = Some(cells);
                    continue;
                }
            }
            println!("reached a fix point");
            if STEP {
                let _ = tokio::io::stdin().read(&mut b).await.unwrap();
                if b[0] == b'q' {
                    break false;
                } else {
                    continue;
                }
            } else if let Some(pos) = clear_random_blank(&client, &mut board).await {
                updated_cells = Some(HashSet::from([pos]));
            }
        }
    };

    if automated_exit {
        let _ = tokio::io::stdin().read(&mut b).await.unwrap();
    }
    client.close().await.unwrap();
}

async fn clear_random_blank(client: &Client, board: &mut Board) -> Option<(usize, usize)> {
    let mut blanks = Vec::new();
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] == BLANK {
                blanks.push((r, c));
            }
        }
    }
    if blanks.is_empty() {
        return None;
    }

    println!("going RANDOM!!!");

    let r: usize = rand::random();
    let ix = r % blanks.len();
    click(client, [blanks[ix]], MOUSE_BUTTON_LEFT).await;
    Some(blanks[ix])
}

async fn check_all_bombs_flagged_then_clear(client: &Client, board: &mut Board) -> Option<HashSet<(usize, usize)>> {
    let mut nbombs = 0;
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] == BOMBFLAGGED {
                nbombs += 1;
            }
        }
    }

    if nbombs == NBOMBS {
        let mut blanks = Vec::new();
        for r in 0..NROWS {
            for c in 0..NCOLS {
                if board[r][c] == BLANK {
                    blanks.push((r, c));
                }
            }
        }
        click(client, blanks.iter().cloned(), MOUSE_BUTTON_LEFT).await;
        println!("clearing all blanks (we've marked all the bombs)");
        Some(blanks.into_iter().collect())
    } else {
        None
    }
}

async fn click_center_cell(client: &Client) {
    let starting_cell = (NROWS / 2, NCOLS / 2);
    client
        .find(Locator::Id(&format!("{}_{}", starting_cell.0, starting_cell.1)))
        .await
        .expect("middle cell")
        .click()
        .await
        .expect("clickable item");
}

async fn flag(client: &Client, board: &mut Board) -> bool {
    let mut to_flag = HashSet::new();
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] > 0 && blank_and_flagged_surrounding(board, r, c).count() == board[r][c] as usize {
                to_flag.extend(blank_surrounding(board, r, c));
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

async fn flag_harder(client: &Client, board: &mut Board) -> bool {
    let mut to_flag = HashSet::new();
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] <= 0 {
                continue;
            }
            let self_surrounding: HashSet<_> = blank_surrounding(board, r, c).collect();
            let self_remaining = board[r][c] as usize - flagged_surrounding(board, r, c).count();
            for (r, c) in numbered_surrounding(board, r, c) {
                let neighbor_surrounding: HashSet<_> = blank_surrounding(board, r, c).collect();
                let neighbor_remaining = board[r][c] as usize - flagged_surrounding(board, r, c).count();
                if self_remaining > neighbor_remaining {
                    continue;
                }
                let diff: Vec<_> = neighbor_surrounding.difference(&self_surrounding).collect();
                if diff.len() == neighbor_remaining - self_remaining {
                    to_flag.extend(diff);
                }

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
            if board[r][c] <= 0 {
                continue;
            }
            let self_surrounding: HashSet<_> = blank_surrounding(board, r, c).collect();
            let self_remaining = board[r][c] as usize - flagged_surrounding(board, r, c).count();
            for (r, c) in numbered_surrounding(board, r, c) {
                let neighbor_surrounding: HashSet<_> = blank_surrounding(board, r, c).collect();
                let neighbor_remaining = board[r][c] as usize - flagged_surrounding(board, r, c).count();

                if self_surrounding.is_subset(&neighbor_surrounding) && self_remaining == neighbor_remaining {
                    to_clear.extend(neighbor_surrounding.difference(&self_surrounding))
                }
            }
        }
    }

    if to_clear.is_empty() {
        return HashSet::new();
    }

    click(client, to_clear.iter().copied(), MOUSE_BUTTON_LEFT).await;
    to_clear
}

async fn clear_harder(client: &Client, board: &Board) -> HashSet<(usize, usize)> {
    let mut to_clear = HashSet::new();
    for r in 0..NROWS {
        for c in 0..NCOLS {
            if board[r][c] > 0 && flagged_surrounding(board, r, c).count() == board[r][c] as usize {
                to_clear.extend(blank_surrounding(board, r, c));
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
                for (r, c) in surrounding(r, c) {
                    to_update.insert((r, c));
                }
            }
        }
    } else if class == "bombflagged" {
        board[r][c] = BOMBFLAGGED;
    } else if class == "bombrevealed" {
        println!("BOOM");
        std::process::exit(1);
    } else {
        println!("unrecognized class: {class}");
    }
}

fn blank_and_flagged_surrounding(board: &Board, r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    surrounding(r, c)
        .filter(|&(r, c)| {
            let e = board[r][c];
            e == BLANK || e == BOMBFLAGGED
        })
}

fn blank_surrounding(board: &Board, r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    surrounding(r, c)
        .filter(|&(r, c)| board[r][c] == BLANK)
}

fn flagged_surrounding(board: &Board, r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    surrounding(r, c)
        .filter(|&(r, c)| board[r][c] == BOMBFLAGGED)
}

fn numbered_surrounding(board: &Board, r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    surrounding(r, c)
        .filter(|&(r, c)| board[r][c] > 0)
}

fn surrounding(r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> {
    [
        (-1, -1),
        (-1, -0),
        (-1,  1),
        ( 0, -1),
        ( 0,  1),
        ( 1, -1),
        ( 1,  0),
        ( 1,  1),
    ]
        .iter()
        .map(move |(dr, dc)| (r as isize + dr, c as isize + dc))
        .filter(|&(r, c)| r >= 0 && r < NROWS as isize && c >= 0 && c < NCOLS as isize)
        .map(|(r, c)| (r as usize, c as usize))
}
