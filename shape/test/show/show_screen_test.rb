class ShowScreenTest < AcceptanceTest
  def setup = workspace("atelier") # book-workshop: two entrypoints, two files with one note, a three-line try, two outcomes, two why paragraphs

  test "show book-workshop is printed exactly as laid out and writes nothing" do
    show "book-workshop"

    assert_success
    assert_stdout_is expected("show_book_workshop.txt")
    assert_nothing_written
  end
end
